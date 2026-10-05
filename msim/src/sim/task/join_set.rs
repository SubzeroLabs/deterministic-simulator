//! API Compatible implementation of tokio::task::JoinSet

#![allow(missing_docs)]

use std::{
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use futures::{
    future::poll_fn,
    stream::{FuturesUnordered, Stream},
};
use tokio::task::LocalSet;

#[cfg(tokio_unstable)]
use crate::task::Id;
use crate::{
    runtime::Handle,
    task::{AbortHandle, JoinError, JoinHandle},
};

pub struct JoinSet<T> {
    inner: FuturesUnordered<JoinHandle<T>>,
}

impl<T> JoinSet<T> {
    pub fn new() -> Self {
        Self {
            inner: FuturesUnordered::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl<T: 'static> JoinSet<T> {
    pub fn spawn<F>(&mut self, task: F) -> AbortHandle
    where
        F: Future<Output = T>,
        F: Send + 'static,
        T: Send,
    {
        self.insert(crate::task::spawn(task))
    }

    pub fn spawn_on<F>(&mut self, task: F, _handle: &Handle) -> AbortHandle
    where
        F: Future<Output = T>,
        F: Send + 'static,
        T: Send,
    {
        self.insert(crate::task::spawn(task))
    }

    pub fn spawn_blocking<F>(&mut self, task: F) -> AbortHandle
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        self.insert(crate::task::spawn_blocking(task))
    }

    pub fn spawn_local<F>(&mut self, task: F) -> AbortHandle
    where
        F: Future<Output = T>,
        F: 'static,
    {
        self.insert(crate::task::spawn_local(task))
    }

    pub fn spawn_local_on<F>(&mut self, task: F, _local_set: &LocalSet) -> AbortHandle
    where
        F: Future<Output = T>,
        F: 'static,
    {
        self.insert(crate::task::spawn_local(task))
    }

    fn insert(&mut self, jh: JoinHandle<T>) -> AbortHandle {
        let abort = jh.abort_handle();
        self.inner.push(jh);
        abort
    }

    pub async fn join_next(&mut self) -> Option<Result<T, JoinError>> {
        poll_fn(|cx| self.poll_join_next(cx)).await
    }

    /// Tries to join one of the tasks in the set that has completed and return its output.
    ///
    /// Returns `None` if there are no completed tasks, or if the set is empty.
    ///
    /// The poll is driven by a no-op waker, so it neither parks the caller nor leaves a
    /// wakeup owed to one: a task that completes after this returns is observed by the next
    /// poll of the set, which re-registers whatever waker that poll carries. A caller that
    /// wants to be woken on completion must await `join_next` instead.
    pub fn try_join_next(&mut self) -> Option<Result<T, JoinError>> {
        let mut cx = Context::from_waker(Waker::noop());
        match self.poll_join_next(&mut cx) {
            Poll::Ready(res) => res,
            Poll::Pending => None,
        }
    }

    pub async fn shutdown(&mut self) {
        self.abort_all();
        while self.join_next().await.is_some() {}
    }

    pub async fn join_all(mut self) -> Vec<T> {
        let mut output = Vec::with_capacity(self.len());

        while let Some(res) = self.join_next().await {
            match res {
                Ok(t) => output.push(t),
                Err(err) if err.is_panic() => std::panic::resume_unwind(err.into_panic()),
                Err(err) => panic!("{err}"),
            }
        }
        output
    }

    pub fn abort_all(&mut self) {
        self.inner.iter().for_each(|jh| jh.abort());
    }

    pub fn detach_all(&mut self) {
        let mut new_inner = FuturesUnordered::new();
        std::mem::swap(&mut new_inner, &mut self.inner);
    }

    pub fn poll_join_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<T, JoinError>>> {
        let pinned = Pin::new(&mut self.inner);
        pinned.poll_next(cx)
    }
}

impl<T> Drop for JoinSet<T> {
    fn drop(&mut self) {
        self.inner
            .iter()
            .for_each(|join_handle| join_handle.abort());
    }
}

impl<T> fmt::Debug for JoinSet<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JoinSet").field("len", &self.len()).finish()
    }
}

impl<T> Default for JoinSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, F> std::iter::FromIterator<F> for JoinSet<T>
where
    F: Future<Output = T>,
    F: Send + 'static,
    T: Send + 'static,
{
    fn from_iter<I: IntoIterator<Item = F>>(iter: I) -> Self {
        let mut set = Self::new();
        iter.into_iter().for_each(|task| {
            set.spawn(task);
        });
        set
    }
}
