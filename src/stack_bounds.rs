use crate::util::zeroed;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StackBounds {
    pub origin: *mut u8,
    pub bound: *mut u8,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl StackBounds {
    pub unsafe fn new_thread_stack_bounds(thread: libc::pthread_t) {
        let origin = libc::pthread_get_stackaddr_np(thread);
        let size = libc::pthread_get_stacksize_np(thread);
        let bound = origin.add(size);
        Self {
            origin: origin.cast(),
            bound: bound.cast(),
        }
    }
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
impl StackBounds {
    #[cfg(target_os = "openbsd")]
    pub unsafe fn new_thread_stack_bounds(thread: libc::pthread_t) {
        let mut stack: libc::stack_t = zeroed();
        libc::pthread_stackseg_np(thread, &mut stack);
        let origin = stack.ss_sp;
        let bound = stack.origin.sub(stack.ss_size);
        return Self {
            origin: origin.cast(),
            bound: bound.cast(),
        };
    }

    #[cfg(not(target_os = "openbsd"))]
    pub unsafe fn new_thread_stack_bounds(thread: libc::pthread_t) -> Self {
        let mut bound = 0 as *mut libc::c_void;
        let mut stack_size = 0;
        let mut sattr: libc::pthread_attr_t = zeroed();
        libc::pthread_attr_init(&mut sattr);
        #[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
        {
            libc::pthread_attr_get_np(thread, &mut sattr);
        }
        #[cfg(not(any(target_os = "freebsd", target_os = "netbsd")))]
        {
            libc::pthread_getattr_np(thread, &mut sattr);
        }
        let _rc = libc::pthread_attr_getstack(&mut sattr, &mut bound, &mut stack_size);
        libc::pthread_attr_destroy(&mut sattr);
        let origin = bound.add(stack_size);
        Self {
            bound: bound.cast(),
            origin: origin.cast(),
        }
    }
}
