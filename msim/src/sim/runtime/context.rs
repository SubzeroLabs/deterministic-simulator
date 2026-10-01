//! Thread local runtime context
use std::{cell::RefCell, sync::Arc};

use crate::{
    runtime::Handle,
    task::{NodeId, TaskInfo},
};

thread_local! {
    static CONTEXT: RefCell<Option<Handle>> = const { RefCell::new(None) };
    static TASK: RefCell<Option<Arc<TaskInfo>>> = const { RefCell::new(None) };
}

pub(crate) fn current<T>(map: impl FnOnce(&Handle) -> T) -> T {
    CONTEXT.with(move |ctx| map(ctx.borrow().as_ref().expect(MSG)))
}

pub(crate) fn try_current<T>(map: impl FnOnce(&Handle) -> T) -> Option<T> {
    CONTEXT.with(move |ctx| ctx.borrow().as_ref().map(map))
}

/// Returns whether a simulator context is currently entered on this thread.
///
/// This is deliberately infallible: it is called from the `extern "C"` library
/// interceptors, which cannot unwind, and it may run while `CONTEXT` is already
/// mutably borrowed (inside `enter`) or has been destroyed (during thread-local
/// teardown). Both cases mean "no usable context", so they answer `false`
/// instead of panicking.
pub(crate) fn has_context() -> bool {
    CONTEXT
        .try_with(|ctx| ctx.try_borrow().map(|c| c.is_some()).unwrap_or(false))
        .unwrap_or(false)
}

pub(crate) fn try_current_task() -> Option<Arc<TaskInfo>> {
    TASK.with(|task| task.borrow().clone())
}

pub(crate) fn current_node() -> NodeId {
    TASK.with(|task| task.borrow().as_ref().expect(MSG).node())
}

/// Set this [`Handle`] as the current active [`Handle`].
///
/// [`Handle`]: Handle
pub(crate) fn enter(new: Handle) -> EnterGuard {
    CONTEXT.with(|ctx| {
        let old = ctx.borrow_mut().replace(new);
        EnterGuard(old)
    })
}

pub(crate) struct EnterGuard(Option<Handle>);

impl Drop for EnterGuard {
    fn drop(&mut self) {
        CONTEXT.with(|ctx| {
            *ctx.borrow_mut() = self.0.take();
        });
    }
}

pub(crate) fn enter_task(new: Arc<TaskInfo>) -> TaskEnterGuard {
    TASK.with(|ctx| {
        let _span = new.span().entered();
        let old = ctx.borrow_mut().replace(new);
        TaskEnterGuard { old, _span }
    })
}

pub(crate) struct TaskEnterGuard {
    old: Option<Arc<TaskInfo>>,
    _span: tracing::span::EnteredSpan,
}

impl Drop for TaskEnterGuard {
    fn drop(&mut self) {
        TASK.with(|ctx| {
            *ctx.borrow_mut() = self.old.take();
        });
    }
}

const MSG: &str =
    "there is no reactor running, must be called from the context of a Madsim runtime";
