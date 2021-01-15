#[cfg(feature = "threaded")]
mod sync {

    use core::sync::atomic::{AtomicI8, Ordering};
    use core::{cell::UnsafeCell, debug_assert_ne};
    use parking_lot::Mutex;

    pub struct TLSState {
        pub safepoint: *mut usize,
        // Whether it is safe to execute GC at the same time.
        pub gc_state: i8,
        //pub alloc: *mut ThreadLocalAllocator,
        pub current_block: Option<BlockTuple>,
        pub current_ovf_block: Option<BlockTuple>,
        pub stack_bottom: *mut u8,
        pub stack_end: *mut u8,
    }
    // gc_state = 1 means the thread is doing GC or is waiting for the GC to
    //              finish.
    pub const GC_STATE_WAITING: i8 = 1;
    // gc_state = 2 means the thread is running unmanaged code that can be
    //              execute at the same time with the GC.
    pub const GC_STATE_SAFE: i8 = 2;

    impl TLSState {
        pub fn atomic_gc_state(&self) -> &AtomicI8 {
            as_atomic!(&self.gc_state;AtomicI8)
        }
        #[inline(always)]
        pub fn yieldpoint(&mut self) {
            unsafe {
                debug_assert_ne!(self.safepoint, 0 as *mut usize);
                core::sync::atomic::fence(Ordering::SeqCst);
                self.stack_end = get_sp!() as *mut u8;
                core::ptr::read_volatile(self.safepoint);
                core::sync::atomic::fence(Ordering::SeqCst);
            }
        }

        #[doc(hidden)]
        #[inline(always)]
        pub fn gc_state_set(&mut self, state: i8, old_state: i8) -> i8 {
            self.atomic_gc_state().store(state, Ordering::Release);
            if old_state != 0 && state == 0 {
                self.yieldpoint();
            }
            old_state
        }

        #[doc(hidden)]
        #[inline(always)]
        pub fn gc_state_save_and_set(&mut self, state: i8) -> i8 {
            self.gc_state_set(state, self.gc_state)
        }
    }
    #[thread_local]
    static TLS: UnsafeCell<TLSState> = {
        UnsafeCell::new(TLSState {
            safepoint: 0 as *mut usize,
            gc_state: 0,
            current_block: None, //alloc: 0 as *mut _,
            current_ovf_block: None,
            stack_bottom: 0 as *mut _,
            stack_end: 0 as *mut _,
        })
    };
    #[no_mangle]
    #[inline]
    pub extern "C" fn immix_prepare_thread(sp: *mut usize) {
        immix_get_tls_state().safepoint = unsafe { crate::safepoint::SAFEPOINT_PAGE as *mut usize };
        immix_get_tls_state().stack_bottom = sp as *mut _;
    }
    #[no_mangle]
    #[inline]
    pub(crate) extern "C" fn immix_get_tls_state() -> &'static mut TLSState {
        unsafe { &mut *TLS.get() }
    }
    /// Checks if current thread should yield. GC won't be able to stop a mutator unless this function is put into code.
    ///
    /// # Performance overhead
    /// This function is no-op when libimmixcons was built without `threaded` feature. When `threaded` feature is enabled
    /// this will emit volatile load without any conditional jumps so it is very small overhead compared to conditional yieldpoints.
    #[inline(always)]
    #[no_mangle]
    pub extern "C" fn immix_mutator_yieldpoint() {
        immix_get_tls_state().yieldpoint();
    }
    use alloc::vec::Vec;

    use crate::{
        allocation::BlockTuple, safepoint::safepoint_init, signals::install_default_signal_handlers,
    };
    pub struct Threads {
        pub threads: Mutex<Vec<*mut TLSState>>,
    }

    impl Threads {
        pub fn new() -> Self {
            Self {
                threads: Mutex::new(Vec::with_capacity(2)),
            }
        }
    }
    unsafe impl Sync for Threads {}
    unsafe impl Send for Threads {}
    pub static THREADS: once_cell::sync::Lazy<Threads> =
        once_cell::sync::Lazy::new(|| Threads::new());
    static HAS_MAIN: AtomicI8 = AtomicI8::new(0);
    /// Registers main thread.
    ///
    /// # Panics
    /// Panics if main thread is already registered.
    ///
    ///
    #[no_mangle]
    pub extern "C" fn immix_register_main_thread(dummy_sp: *mut u8) {
        assert!(
            HAS_MAIN.load(Ordering::Relaxed) != 1,
            "main thread already registered"
        );
        unsafe {
            safepoint_init();
        }

        install_default_signal_handlers();
        let mut lock = THREADS.threads.lock();
        immix_prepare_thread(dummy_sp.cast());
        immix_get_tls_state().stack_bottom = dummy_sp;
        //let tls = get_tls_state() as *mut _;
        lock.push(immix_get_tls_state() as *mut _);
    }
    /// Register thread.
    /// ## Inputs
    /// `sp`: pointer to variable on stack for searching roots on stack.
    ///
    #[no_mangle]
    pub extern "C" fn immix_register_thread(sp: *mut usize) {
        let threads = &*THREADS;
        let mut lock = threads.threads.lock();
        immix_prepare_thread(sp);

        lock.push(immix_get_tls_state() as *mut _);
    }
    /// Unregister thread.
    #[no_mangle]
    pub extern "C" fn immix_unregister_thread() {
        let threads = &*THREADS;
        let tls = immix_get_tls_state();
        let mut lock = threads.threads.lock();
        lock.retain(|x| *x != tls);
    }
    /// Enter unsafe GC state. This means current thread runs "managed by GC code" and GC *must* stop this thread
    /// at GC cycle.
    ///
    /// Returns current state to restore later.
    pub extern "C" fn immix_unsafe_enter() -> i8 {
        immix_get_tls_state().gc_state_save_and_set(0)
    }
    /// Leave unsafe GC state and restore previous state from `state` argument. This function has yieldpoint internally so thread
    /// might be suspended for GC.
    pub extern "C" fn immix_unsafe_leave(state: i8) -> i8 {
        immix_get_tls_state().gc_state_set(state, 0)
    }
    /// Enter safe for GC state. When thread is in safe state it is allowed to execute code at the same time with the GC.
    ///
    ///
    /// Returns current state to restore later.
    pub extern "C" fn immix_safe_enter() -> i8 {
        immix_get_tls_state().gc_state_save_and_set(GC_STATE_SAFE)
    }
    /// Leave safe for GC state and restore previous state from `state` argument.
    pub extern "C" fn immix_safe_leave(state: i8) -> i8 {
        immix_get_tls_state().gc_state_set(state, GC_STATE_SAFE)
    }
    pub(crate) fn set_gc_and_wait() {
        let ptls = immix_get_tls_state();
        let state = ptls.gc_state;
        ptls.atomic_gc_state()
            .store(GC_STATE_WAITING, Ordering::Release);
        crate::safepoint::safepoint_wait_gc();
        ptls.atomic_gc_state().store(state, Ordering::Release);
    }
}
#[cfg(not(feature = "threaded"))]
mod unsync {
    pub(crate) fn set_gc_and_wait() {
        /* no-op */
    }
    /// Registers main thread.
    ///
    /// # Panics
    /// Panics if main thread is already registered.
    ///
    ///
    #[no_mangle]
    pub extern "C" fn immix_register_main_thread() {
        /* no-op */
    }
    /// Register thread.
    /// ## Inputs
    /// `sp`: pointer to variable on stack for searching roots on stack.
    ///
    #[no_mangle]
    pub extern "C" fn immix_register_thread() {
        /* no-op */
    }
    /// Unregister thread.
    #[no_mangle]
    pub extern "C" fn immix_unregister_thread() {
        /* no-op */
    }
    use core::cell::UnsafeCell;
    static mut TLS: UnsafeCell<TLSState> = UnsafeCell::new(TLSState);
    /// Checks if current thread should yield. GC won't be able to stop a thread unless this function is put into code.
    ///
    /// # Performance overhead
    /// This function is no-op when libimmixcons was built without `threaded` feature. When `threaded` feature is enabled
    /// this will emit volatile load without any conditional jumps so it is very small overhead compared to conditional yieldpoints.
    #[inline(always)]
    #[no_mangle]
    pub extern "C" fn immix_mutator_yieldpoint() {}
    #[no_mangle]
    #[inline]
    pub(crate) extern "C" fn immix_get_tls_state() -> &'static mut TLSState {
        unsafe { &mut *TLS.get() }
    }
    #[inline]
    pub extern "C" fn immix_prepare_thread() {
        /* no-op */
    }
    /// Enter unsafe GC state. This means current thread runs "managed by GC code" and GC *must* stop this thread
    /// at GC cycle.
    ///
    /// Returns current state to restore later.
    pub extern "C" fn immix_unsafe_enter() -> i8 {
        0
    }
    /// Leave unsafe GC state and restore previous state from `state` argument. This function has yieldpoint internally so thread
    /// might be suspended for GC.
    pub extern "C" fn immix_unsafe_leave(state: i8) -> i8 {
        state
    }
    /// Enter safe for GC state. When thread is in safe state it is allowed to execute code at the same time with the GC.
    ///
    ///
    /// Returns current state to restore later.
    pub extern "C" fn immix_safe_enter() -> i8 {
        0
    }
    /// Leave safe for GC state and restore previous state from `state` argument.
    pub extern "C" fn immix_safe_leave(state: i8) -> i8 {
        state
    }
    pub struct TLSState;
    impl TLSState {
        #[inline(always)]
        pub fn yieldpoint(&self) {}
        #[doc(hidden)]
        #[inline(always)]
        pub fn gc_state_set(&self, _state: i8, _old_state: i8) -> i8 {
            0
        }

        #[doc(hidden)]
        #[inline(always)]
        pub fn gc_state_save_and_set(&self, _state: i8) -> i8 {
            0
        }
    }
}

#[cfg(feature = "threaded")]
pub use sync::*;

#[cfg(not(feature = "threaded"))]
pub use unsync::*;
