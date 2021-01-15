#[cfg(feature = "threaded")]
use crate::safepoint::*;
use crate::threading::*;
use crate::util::*;
use core::ptr::*;
use libc::*;
#[cfg(target_os = "macos")]
pub const MAP_ANONYMOUS: c_int = MAP_ANON;

unsafe extern "C" fn sigdie_handler(sig: i32, _info: *mut siginfo_t, _context: *mut c_void) {
    let mut sset = zeroed::<sigset_t>();
    sigfillset(&mut sset);
    sigprocmask(SIG_UNBLOCK, &mut sset, null_mut());
    signal(sig, SIG_DFL);

    if sig != SIGSEGV && sig != SIGBUS && sig != SIGILL {
        raise(sig);
    }

    // fall-through return to re-execute faulting statement (but without the error handler)
}

unsafe extern "C" fn segv_handler(sig: i32, info: *mut siginfo_t, context: *mut c_void) {
    #[cfg(feature = "threaded")]
    {
        if addr_in_safepoint((&*info).si_addr() as _) {
            debug!(
                "Stopped thread at {:p} for GC",
                crate::threading::immix_get_tls_state()
            );
            set_gc_and_wait();
            debug!(
                "Thread at {:p} resumed",
                crate::threading::immix_get_tls_state()
            );
            return;
        }
    }

    sigdie_handler(sig, info, context);
}

unsafe fn allocate_segv_handler() {
    let mut act: sigaction = zeroed();
    sigemptyset(&mut act.sa_mask);
    act.sa_sigaction = segv_handler as _;
    act.sa_flags = SA_ONSTACK | SA_SIGINFO;
    if sigaction(SIGSEGV, &act, null_mut()) < 0 {
        panic!("fatal error: sigaction",);
    }
    // On AArch64, stack overflow triggers a SIGBUS
    if sigaction(SIGBUS, &act, null_mut()) < 0 {
        panic!("fatal error: sigaction");
    }
}

pub fn install_default_signal_handlers() {
    unsafe {
        allocate_segv_handler();
    }
}
