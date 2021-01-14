use crate::object::*;
use crate::util::*;
use core::alloc::Layout;
use core::sync::atomic::AtomicBool;
use core::{alloc::Allocator, ptr::NonNull};
/// Precise allocation used for large objects (>= LARGE_CUTOFF).
/// Wafflelink uses GlobalAlloc that already knows what to do for large allocations. The GC shouldn't
/// have to think about such things. That's where PreciseAllocation comes in. We will allocate large
/// objects directly using malloc, and put the PreciseAllocation header just before them. We can detect
/// when a *mut Object is a PreciseAllocation because it will have the ATOM_SIZE / 2 bit set.
#[repr(C)]
pub struct PreciseAllocation {
    //pub link: LinkedListLink,
    /// allocation request size
    pub cell_size: usize,
    /// Is this allocation marked by GC?
    pub is_marked: bool,
    /// index in precise_allocations
    pub index_in_space: u32,
    /// Is alignment adjusted?
    //pub is_newly_allocated: bool,
    pub adjusted_alignment: bool,
    /// Is this even valid allocation?
    pub has_valid_cell: bool,
}

impl PreciseAllocation {
    /// Return atomic reference to `is_marked`
    pub fn mark_atomic(&self) -> &AtomicBool {
        unsafe { &*(&self.is_marked as *const bool as *const AtomicBool) }
    }
    /// Alignment of allocation.
    pub const ALIGNMENT: usize = 16;
    /// Alignment of pointer returned by `Self::cell`.
    pub const HALF_ALIGNMENT: usize = Self::ALIGNMENT / 2;
    /// Check if raw_ptr is precisely allocated.
    pub fn is_precise(raw_ptr: *mut ()) -> bool {
        (raw_ptr as usize & Self::HALF_ALIGNMENT) != 0
    }
    /// Create PreciseAllocation from pointer
    pub fn from_cell(ptr: *mut RawGc) -> *mut Self {
        unsafe {
            ptr.cast::<u8>()
                .offset(-(Self::header_size() as isize))
                .cast()
        }
    }
    /// Return base pointer
    #[inline]
    pub fn base_pointer(&self) -> *mut () {
        if self.adjusted_alignment {
            ((self as *const Self as isize) - (Self::HALF_ALIGNMENT as isize)) as *mut ()
        } else {
            self as *const Self as *mut ()
        }
    }
    /// Set `is_marked` to false
    pub fn clear_marked(&mut self) {
        self.is_marked = false;
        //self.is_marked.store(false, Ordering::Relaxed);
    }
    /// Return `is_marked`
    pub fn is_marked(&self) -> bool {
        self.is_marked
    }
    /// Test and set marked. Will return true
    /// if it is already marked.
    pub fn test_and_set_marked(&mut self) -> bool {
        if self.is_marked() {
            return true;
        }
        self.is_marked = true;
        false
    }

    /// Return cell address, it is always aligned to `Self::HALF_ALIGNMENT`.
    pub fn cell(&self) -> *mut RawGc {
        let addr = Address::from_ptr(self).offset(Self::header_size());
        addr.to_mut_ptr()
    }
    /// Return true if raw_ptr is above lower bound
    pub fn above_lower_bound(&self, raw_ptr: *mut ()) -> bool {
        let ptr = raw_ptr;
        let begin = self.cell() as *mut ();
        ptr >= begin
    }
    /// Return true if raw_ptr below upper bound
    pub fn below_upper_bound(&self, raw_ptr: *mut ()) -> bool {
        let ptr = raw_ptr;
        let begin = self.cell() as *mut ();
        let end = (begin as usize + self.cell_size) as *mut ();
        ptr <= (end as usize + 8) as *mut ()
    }
    /// Returns header size + required alignment to make cell be aligned to 8.
    pub const fn header_size() -> usize {
        ((core::mem::size_of::<PreciseAllocation>() + Self::HALF_ALIGNMENT - 1)
            & !(Self::HALF_ALIGNMENT - 1))
            | Self::HALF_ALIGNMENT
    }
    /// Does this allocation contains raw_ptr?
    pub fn contains(&self, raw_ptr: *mut ()) -> bool {
        self.above_lower_bound(raw_ptr) && self.below_upper_bound(raw_ptr)
    }
    /// Is this allocation live?
    pub fn is_live(&self) -> bool {
        self.is_marked() //|| self.is_newly_allocated
    }
    /// Clear mark bit
    pub fn flip(&mut self) {
        self.clear_marked();
    }
    /// Is this marked?
    pub fn is_empty(&self) -> bool {
        !self.is_marked() //&& !self.is_newly_allocated
    }
    /// Derop cell if this allocation is not marked.
    pub fn sweep(&mut self) -> bool {
        if self.has_valid_cell && !self.is_live() {
            self.has_valid_cell = false;
            let cell = self.cell();
            unsafe {
                if let Some(fin) = (*cell).rtti().finalizer {
                    fin(cell as *mut u8);
                }
            }
            return false;
        }
        true
    }
    /// Try to create precise allocation (no way that it will return null for now).
    pub fn try_create(size: usize, index_in_space: u32) -> *mut Self {
        let adjusted_alignment_allocation_size = Self::header_size() + size + Self::HALF_ALIGNMENT;
        unsafe {
            let mut space = LibcAlloc
                .allocate(
                    Layout::from_size_align(
                        adjusted_alignment_allocation_size,
                        Self::HALF_ALIGNMENT,
                    )
                    .unwrap(),
                )
                .unwrap()
                .cast::<u8>()
                .as_ptr();
            //let mut space = libc::malloc(adjusted_alignment_allocation_size);
            let mut adjusted_alignment = false;
            if !is_aligned_for_precise_allocation(space) {
                space = space.add(Self::HALF_ALIGNMENT);
                adjusted_alignment = true;
                assert!(is_aligned_for_precise_allocation(space));
            }
            assert!(size != 0);
            space.cast::<Self>().write(Self {
                //link: LinkedListLink::new(),
                adjusted_alignment,
                is_marked: false,
                //is_newly_allocated: true,
                has_valid_cell: true,
                cell_size: size,
                index_in_space,
            });

            assert!((&*(&*space.cast::<Self>()).cell()).is_precise_allocation());
            space.cast()
        }
    }
    /// return cell size
    pub fn cell_size(&self) -> usize {
        self.cell_size
    }
    /// Destroy this allocation
    pub fn destroy(&mut self) {
        let adjusted_alignment_allocation_size =
            Self::header_size() + self.cell_size + Self::HALF_ALIGNMENT;
        let base = self.base_pointer();
        unsafe {
            LibcAlloc.deallocate(
                NonNull::new_unchecked(base.cast::<u8>()),
                Layout::from_size_align(adjusted_alignment_allocation_size, Self::ALIGNMENT)
                    .unwrap(),
            );
        }
    }
}
/// Check if `mem` is aligned for precise allocation
pub fn is_aligned_for_precise_allocation(mem: *mut u8) -> bool {
    let allocable_ptr = mem as usize;
    (allocable_ptr & (PreciseAllocation::ALIGNMENT - 1)) == 0
}
/// This space contains objects which are larger than the size limits of other spaces.
/// Each object gets its own malloc'd region of memory.
/// Large objects are never moved by the garbage collector.
pub struct LargeObjectSpace {
    allocations: alloc::vec::Vec<*mut PreciseAllocation, LibcAlloc>,
    pub(crate) current_live_mark: bool,
}

impl Default for LargeObjectSpace {
    fn default() -> Self {
        Self::new()
    }
}

impl LargeObjectSpace {
    pub fn new() -> Self {
        Self {
            current_live_mark: false,
            allocations: alloc::vec::Vec::with_capacity_in(8, LibcAlloc),
        }
    }
    pub fn sweep(&mut self) -> usize {
        let mut sweeped = 0;
        self.allocations.retain(|ptr| unsafe {
            let p = &mut **ptr;
            let retain = p.sweep();
            if !retain {
                p.destroy();
                sweeped += p.cell_size;
            }
            retain
        });
        self.allocations.sort_unstable();
        sweeped
    }
    #[allow(clippy::collapsible_if)]
    pub fn contains(&self, p: Address) -> bool {
        if self.allocations.is_empty() {
            return false;
        }
        unsafe {
            if (&*self.allocations[0]).above_lower_bound(p.to_mut_ptr())
                && (&**self.allocations.last().unwrap()).below_upper_bound(p.to_mut_ptr())
            {
                if self
                    .allocations
                    .binary_search(&PreciseAllocation::from_cell(p.to_mut_ptr()))
                    .is_ok()
                {
                    return true;
                }
            }
        }
        false
    }

    pub fn alloc(&mut self, size: usize, vtable: usize) -> Address {
        let ix = self.allocations.len() as u32;
        let cell = PreciseAllocation::try_create(size, ix);
        unsafe {
            if cell.is_null() {
                return Address::null();
            }
            self.allocations.push(cell);
            if (cell as usize) < self.allocations[self.allocations.len() - 1] as usize
                || (cell as usize) < self.allocations[0] as usize
            {
                self.allocations.sort_unstable();
            }
            let raw = (&*cell).cell();
            raw.write(RawGc::new(vtable));
            (&mut *raw).mark(self.current_live_mark);
            Address::from_ptr(raw)
        }
    }
}

impl Drop for LargeObjectSpace {
    fn drop(&mut self) {
        while let Some(alloc) = self.allocations.pop() {
            unsafe {
                (&mut *alloc).destroy();
            }
        }
    }
}
