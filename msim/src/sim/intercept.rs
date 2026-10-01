use tracing::info;

// This is called when a `Runtime` is constructed. Interception itself is scoped to the
// simulator context (see `intercepts_enabled`), not to the constructing thread, so this
// only records that a simulator now exists in the process.
pub(crate) fn enable_intercepts(e: bool) {
    let cur_thread = std::thread::current().id();
    info!(
        "{} library call intercepts (context-scoped), runtime built on thread {:?}",
        if e { "enabling" } else { "disabling" },
        cur_thread
    );
}

/// Whether the library call interceptors should redirect into the simulator.
///
/// This follows the simulator *context*, not the thread that happened to construct the
/// `Runtime`. `Runtime::block_on` and `Handle::enter` install the context on whichever
/// thread calls them, so a runtime built on one thread and driven on another is still
/// fully intercepted. Conversely, code running outside any entered context - including
/// on the constructing thread before `block_on`, after it returns, or after the runtime
/// is dropped - reaches the real libc, so real IO on unrelated threads is unaffected and
/// a stray `close()` cannot fault looking for a simulator that is not there.
pub(crate) fn intercepts_enabled() -> bool {
    crate::context::has_context()
}

/// Cache and call a library function via dlsym()
#[macro_export]
macro_rules! define_sys_interceptor {

    (fn $name:ident ( $($param:ident : $type:ty),* $(,)? ) -> $ret:ty { $($body:tt)+ }) => {

        #[no_mangle]
        #[inline(never)]
        unsafe extern "C" fn $name ( $($param: $type),* ) -> $ret {
            lazy_static::lazy_static! {
                static ref NEXT_DL_SYM: unsafe extern "C" fn ( $($param: $type),* ) -> $ret = unsafe {

                    // Can't use CString::new because it allocates, and allocators can call system
                    // functions...
                    let fn_name_c = concat!(stringify!($name), "\0");

                    let ptr = libc::dlsym(libc::RTLD_NEXT, fn_name_c.as_ptr() as _);
                    assert!(!ptr.is_null(), "{:?}", fn_name_c);
                    std::mem::transmute(ptr)
                };
            }

            if !$crate::sim::intercept::intercepts_enabled() {
                return NEXT_DL_SYM($($param),*);
            }

            $($body)*
        }
    }
}

/// define a function that can be used to bypass a interception (as defined by
/// define_sys_interceptor.
#[macro_export]
macro_rules! define_bypass {
    ($name:ident, fn $cname:ident ( $($param:ident : $type:ty),* $(,)? ) -> $ret:ty) => {
        unsafe fn $name ( $($param: $type),* ) -> $ret {
            lazy_static::lazy_static! {
                static ref NEXT_DL_SYM: unsafe extern "C" fn ( $($param: $type),* ) -> $ret = unsafe {

                    // Can't use CString::new because it allocates, and allocators can call system
                    // functions...
                    let fn_name_c = concat!(stringify!($cname), "\0");

                    let ptr = libc::dlsym(libc::RTLD_NEXT, fn_name_c.as_ptr() as _);
                    assert!(!ptr.is_null());
                    std::mem::transmute(ptr)
                };
            }

            return NEXT_DL_SYM($($param),*);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant as StdInstant};

    use crate::runtime::Runtime;

    /// Interception must follow the simulator context, not the thread that constructed the
    /// `Runtime`. A runtime built on one thread and driven on another used to fall through to
    /// the real clock, so `std::time` inside the simulation saw host time and the run was no
    /// longer reproducible from its seed.
    #[test]
    fn intercepts_follow_the_context_not_the_constructing_thread() {
        // built here, driven on the thread below
        let runtime = Runtime::with_seed(42);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                runtime.block_on(async {
                    let started = StdInstant::now();
                    crate::time::sleep(Duration::from_secs(5)).await;
                    // the simulated clock jumped 5s, so std must observe 5s too
                    assert!(
                        started.elapsed() >= Duration::from_secs(5),
                        "std::time::Instant saw {:?} across a 5s simulated sleep, \
                         meaning it read the host clock instead of the simulated one",
                        started.elapsed()
                    );
                });
            });
        });
    }

    /// Outside an entered context the interceptors must fall through to the real libc.
    /// Otherwise a `close()` on the constructing thread - before `block_on`, after it
    /// returns, or after the runtime is dropped - looks for a simulator that is not there.
    #[test]
    fn intercepts_are_disabled_outside_a_context() {
        assert!(!super::intercepts_enabled(), "no runtime exists yet");
        let runtime = Runtime::with_seed(7);
        assert!(
            !super::intercepts_enabled(),
            "constructing a Runtime must not arm interception outside its context"
        );
        runtime.block_on(async {
            assert!(
                super::intercepts_enabled(),
                "inside block_on the context is entered"
            );
        });
        assert!(
            !super::intercepts_enabled(),
            "leaving block_on must disarm interception again"
        );
        drop(runtime);
        assert!(!super::intercepts_enabled(), "runtime dropped");
    }
}
