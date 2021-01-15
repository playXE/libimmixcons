use std::ops::{Deref, DerefMut};

#[link(name = "gc")]
extern "C" {
    pub fn GC_malloc(size: usize) -> *mut u8;
    pub fn GC_init();
    pub fn GC_enable_incremental();
}

pub struct Gc<T> {
    ptr: *mut T,
}

impl<T> Deref for Gc<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl<T> DerefMut for Gc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.ptr }
    }
}

#[derive(Copy, Clone)]
pub struct Heap;

impl Heap {
    pub fn new() -> Self {
        unsafe {
            GC_init();
        }
        Self
    }

    pub fn allocate<T>(&mut self, value: T) -> Gc<T> {
        Gc {
            ptr: unsafe {
                let p = GC_malloc(std::mem::size_of::<T>()).cast::<T>();
                p.write(value);
                p
            },
        }
    }
}

impl<T> Clone for Gc<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Gc<T> {}
