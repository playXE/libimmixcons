use super::block::ImmixBlock;
use super::constants::*;
use crate::util::{Address, LibcAlloc};
#[cfg(windows)]
mod _win {
    use super::*;
    use core::{ptr::null_mut, usize};

    use winapi::um::{
        memoryapi::{VirtualAlloc, VirtualFree},
        winnt::{MEM_COMMIT, MEM_DECOMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE},
    };

    pub struct Mmap {
        start: *mut u8,
        end: *mut u8,
        size: usize,
    }
    impl Mmap {
        pub fn new(size: usize) -> Self {
            unsafe {
                let mem = VirtualAlloc(null_mut(), size, MEM_RESERVE, PAGE_READWRITE);
                let mem = mem as *mut u8;

                let end = mem.add(size);

                Self {
                    start: mem,
                    end,
                    size,
                }
            }
        }
        /// Return a `BLOCK_SIZE` aligned pointer to the mmap'ed region.
        pub fn aligned(&self) -> *mut u8 {
            let offset = BLOCK_SIZE - (self.start as usize) % BLOCK_SIZE;
            unsafe { self.start.add(offset) as *mut u8 }
        }

        pub fn start(&self) -> *mut u8 {
            self.start
        }
        pub fn end(&self) -> *mut u8 {
            self.end
        }

        pub fn dontneed(&self, page: *mut u8, size: usize) {
            unsafe {
                VirtualFree(page.cast(), size, MEM_DECOMMIT);
            }
        }

        pub fn commit(&self, page: *mut u8, size: usize) {
            unsafe {
                VirtualAlloc(page.cast(), size, MEM_COMMIT, PAGE_READWRITE);
            }
        }
    }

    impl Drop for Mmap {
        fn drop(&mut self) {
            unsafe {
                VirtualFree(self.start.cast(), self.size, MEM_RELEASE);
            }
        }
    }
}

#[cfg(unix)]
mod _unix {
    use super::*;
    pub struct Mmap {
        start: *mut u8,
        end: *mut u8,
        size: usize,
    }

    impl Mmap {
        pub fn new(size: usize) -> Self {
            unsafe {
                let map = libc::mmap(
                    core::ptr::null_mut(),
                    size as _,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANON,
                    -1,
                    0,
                );
                if map == libc::MAP_FAILED {
                    panic!("mmap failed");
                }
                Self {
                    start: map as *mut u8,
                    end: (map as usize + size) as *mut u8,
                    size,
                }
            }
        }
        /// Return a `BLOCK_SIZE` aligned pointer to the mmap'ed region.
        pub fn aligned(&self) -> *mut u8 {
            let offset = BLOCK_SIZE - (self.start as usize) % BLOCK_SIZE;
            unsafe { self.start.offset(offset as isize) as *mut u8 }
        }

        pub fn start(&self) -> *mut u8 {
            self.start
        }
        pub fn end(&self) -> *mut u8 {
            self.end
        }

        pub fn dontneed(&self, page: *mut u8, size: usize) {
            unsafe {
                libc::madvise(page as *mut _, size as _, libc::MADV_DONTNEED);
            }
        }

        pub fn commit(&self, page: *mut u8, size: usize) {
            unsafe {
                libc::madvise(page as *mut _, size as _, libc::MADV_WILLNEED);
            }
        }
    }

    impl Drop for Mmap {
        fn drop(&mut self) {
            unsafe {
                libc::munmap(self.start() as *mut _, self.size as _);
            }
        }
    }
}

#[cfg(unix)]
pub use _unix::*;
#[cfg(windows)]
pub use _win::*;
use atomic::Ordering;
#[cfg(feature = "threaded")]
use parking_lot::lock_api::RawMutex;
pub struct BlockAllocator {
    #[cfg(feature = "threaded")]
    lock: parking_lot::RawMutex,
    free_blocks: alloc::vec::Vec<*mut ImmixBlock, LibcAlloc>,

    //pub bitmap: SpaceBitmap<16>,
    pub data_bound: *mut u8,
    pub data: *mut u8,
    pub mmap: Mmap,
}

impl BlockAllocator {
    pub fn total_blocks(&self) -> usize {
        (self.mmap.end() as usize - self.mmap.aligned() as usize) / BLOCK_SIZE
    }
    pub fn new(size: usize) -> BlockAllocator {
        let map = Mmap::new(size);
        debug!(
            "New immix space from {:p} to {:p} ({})",
            map.aligned(),
            map.end(),
            crate::formatted_size(map.end() as usize - map.aligned() as usize)
        );
        let this = Self {
            #[cfg(feature = "threaded")]
            lock: parking_lot::RawMutex::INIT,
            data: map.aligned(),
            data_bound: map.end(),
            free_blocks: alloc::vec::Vec::new_in(LibcAlloc),

            mmap: map,
        };
        debug_assert!(this.data as usize % BLOCK_SIZE == 0);
        this
    }

    /// Get a new block aligned to `BLOCK_SIZE`.
    pub fn get_block(&mut self) -> Option<*mut ImmixBlock> {
        if self.free_blocks.is_empty() {
            return self.build_block();
        }
        self.lock.lock();
        let block = self
            .free_blocks
            .pop()
            .map(|x| {
                self.mmap.commit(x as *mut u8, BLOCK_SIZE);
                x
            })
            .or_else(|| self.build_block());
        unsafe { self.lock.unlock() };
        block
    }

    pub fn is_in_space(&self, object: Address) -> bool {
        self.mmap.start() < object.to_mut_ptr() && object.to_mut_ptr() <= self.data_bound
    }
    #[allow(unused_unsafe)]
    fn build_block(&mut self) -> Option<*mut ImmixBlock> {
        unsafe {
            let data = as_atomic!(&self.data;AtomicUsize);
            let mut old = data.load(Ordering::Relaxed);
            let mut new;
            loop {
                new = old + BLOCK_SIZE;
                if new > self.data_bound as usize {
                    return None;
                }
                let res = data.compare_exchange_weak(old, new, Ordering::SeqCst, Ordering::Relaxed);
                match res {
                    Ok(_) => break,
                    Err(x) => old = x,
                }
            }
            debug_assert!(old % BLOCK_SIZE == 0, "block is not aligned for block_size");
            self.mmap.commit(old as *mut u8, BLOCK_SIZE);
            Some(old as *mut ImmixBlock)
        }
    }

    /// Return a collection of blocks.
    pub fn return_blocks(&mut self, blocks: impl IntoIterator<Item = *mut ImmixBlock>) {
        self.lock.lock();
        let iter = blocks.into_iter();

        iter.for_each(|block| {
            self.mmap.dontneed(block as *mut u8, BLOCK_SIZE); // MADV_DONTNEED or MEM_DECOMMIT
            self.free_blocks.push(block);
        });

        unsafe { self.lock.unlock() }
    }

    /// Return the number of unallocated blocks.
    pub fn available_blocks(&self) -> usize {
        let nblocks = ((self.data_bound as usize) - (self.data as usize)) / BLOCK_SIZE;

        nblocks + self.free_blocks.len()
    }
}