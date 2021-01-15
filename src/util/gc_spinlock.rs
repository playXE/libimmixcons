use super::VolatileCell;
use crate::{thread_self, threading::immix_mutator_yieldpoint};
use core::sync::atomic::*;
#[repr(C)]
pub struct GCSpinLock {
    pub owner: VolatileCell<u64>,
    pub count: VolatileCell<u32>,
}

impl GCSpinLock {
    pub fn new() -> Self {
        Self {
            owner: VolatileCell::new(0),
            count: VolatileCell::new(0),
        }
    }
    fn wait(&self, safepoint: bool) {
        let this = thread_self();
        let mut owner = as_atomic!(&self.owner; AtomicU64).load(Ordering::Relaxed);
        if owner == this {
            self.count.set(self.count.get() + 1);
            return;
        }
        loop {
            if owner == 0
                && as_atomic!(&self.owner; AtomicU64).compare_exchange(
                    0,
                    this,
                    Ordering::SeqCst,
                    Ordering::Relaxed,
                ) == Ok(0)
            {
                self.count.set(1);
                return;
            }
            if safepoint {
                immix_mutator_yieldpoint();
            }
            spin_loop_hint();
            owner = as_atomic!(&self.owner; AtomicU64).load(Ordering::Relaxed);
        }
    }

    pub fn lock_nogc(&self) {
        self.wait(false)
    }

    pub fn lock(&self) {
        self.wait(true);
    }

    pub fn unlock(&self) {
        let c = self.count.get() - 1;
        self.count.set(c);
        if c == 0 {
            as_atomic!(&self.owner;AtomicU64).store(0, Ordering::Release);
        }
    }

    pub fn try_lock(&self) -> bool {
        let this = thread_self();
        let owner = as_atomic!(&self.owner; AtomicU64).load(Ordering::Relaxed);
        if owner == this {
            self.count.set(self.count.get() + 1);
            return true;
        }
        if owner == 0
            && as_atomic!(&self.owner; AtomicU64).compare_exchange(
                0,
                this,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) == Ok(0)
        {
            self.count.set(1);
            return true;
        }

        false
    }
}
