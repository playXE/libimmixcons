#![allow(dead_code)]
#![allow(improper_ctypes_definitions)]
#![feature(
    allocator_api,
    asm,
    nonnull_slice_from_raw_parts,
    alloc_layout_extra,
    raw,
    linked_list_cursors,
    thread_local
)]
#![no_std]

#[cfg(feature = "log")]
#[macro_use]
extern crate log;

use core::ptr::NonNull;

use allocation::ImmixSpace;
use constants::{BLOCK_SIZE, LARGE_OBJECT};
use large_object_space::LargeObjectSpace;
extern crate alloc;

#[macro_use]
pub(crate) mod util;
pub mod allocation;
pub mod block;
pub mod block_allocator;
pub mod collector;
pub mod constants;
pub(crate) mod large_object_space;
pub mod object;
#[cfg(feature = "threaded")]
pub mod safepoint;
pub mod signals;
pub mod space_bitmap;
pub mod threading;
use alloc::collections::LinkedList;
use alloc::vec::Vec;
use collector::Collector;
use large_object_space::PreciseAllocation;
use libc::malloc;
use object::*;
use object::{RawGc, TracerPtr};
#[cfg(feature = "threaded")]
use parking_lot::lock_api::RawMutex;
use threading::immix_get_tls_state;

use util::*;
pub struct Immix {
    los: LargeObjectSpace,
    immix: *mut ImmixSpace,
    stack_bottom: *mut u8,
    stack_end: *mut u8,
    allocated: usize,
    threshold: usize,
    current_live_mark: bool,
    collect_roots_callback: Vec<(CollectRootsCallback, *mut u8)>,

    collector: Collector,
    to_finalize: LinkedList<*mut RawGc>,
    #[cfg(feature = "threaded")]
    fin_lock: parking_lot::RawMutex,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CollectionType {
    ImmixCollection,
    ImmixEvacCollection,
}

pub type CollectRootsCallback =
    extern "C" fn(data: *mut u8, tracer: TracerPtr, cons_tracer: ConservativeTracer);

impl Immix {
    #[inline(never)]
    fn collect_internal(&mut self, evacuation: bool, emergency: bool) {
        unsafe {
            let threads;
            #[cfg(feature = "threaded")]
            {
                immix_get_tls_state().stack_end = get_sp!() as *mut _;
                if !safepoint::safepoint_start_gc() {
                    return;
                }
                threads = safepoint::safepoint_wait_for_the_world();
            };
            #[cfg(not(feature = "threaded"))]
            {
                threads = ();
            }

            let mut precise_roots = Vec::new();
            let mut cons = Vec::new();
            for &(callback, data) in self.collect_roots_callback.iter() {
                struct VisitRoots {
                    v: *mut Vec<*mut *mut RawGc>,
                }

                impl Tracer for VisitRoots {
                    fn trace(&mut self, reference: &mut core::ptr::NonNull<RawGc>) {
                        unsafe {
                            (&mut *self.v).push(core::mem::transmute(reference));
                        }
                    }
                }
                callback(
                    data,
                    TracerPtr {
                        tracer: core::mem::transmute(&mut VisitRoots {
                            v: &mut precise_roots,
                        } as &mut dyn Tracer),
                    },
                    ConservativeTracer { roots: &mut cons },
                );
            }
            let mut roots: Vec<*mut RawGc> = Vec::new();
            let mut all_blocks = (*self.immix).get_all_blocks();
            #[cfg(feature = "threaded")]
            {
                for thread in threads.iter() {
                    let thread = &mut **thread;
                    if let Some(block) = thread.current_block.take() {
                        all_blocks.push(block.0);
                    }
                    if let Some(block) = thread.current_ovf_block.take() {
                        all_blocks.push(block.0);
                    }
                    self.collect_roots(
                        thread.stack_bottom as *mut *mut u8,
                        thread.stack_end as *mut *mut u8,
                        &mut roots,
                    );
                }
            }
            for &(bottom, end) in cons.iter() {
                self.collect_roots(bottom as *mut *mut u8, end as *mut *mut u8, &mut roots);
            }
            #[cfg(not(feature = "threaded"))]
            {
                self.collect_roots(
                    self.stack_bottom as *mut *mut u8,
                    self.stack_end as *mut *mut u8,
                    &mut roots,
                );
            }
            self.collector.extend_all_blocks(all_blocks);
            let collection_type = self.collector.prepare_collection(
                evacuation,
                true,
                (*(*self.immix).block_allocator).available_blocks(),
                (*self.immix).evac_headroom(),
                (*(*self.immix).block_allocator).total_blocks(),
                emergency,
            );

            let visited = self.collector.collect(
                &collection_type,
                &roots,
                &precise_roots,
                &mut *self.immix,
                &mut self.los,
                !self.current_live_mark,
            );
            for root in roots.iter() {
                {
                    (&mut **root).unpin()
                };
            }
            let mut cursor = self.to_finalize.cursor_front_mut();
            while let Some(elem) = cursor.current() {
                if (**elem).get_mark() == self.current_live_mark {
                    if (**elem).rtti().needs_finalization {
                        if let Some(fin) = (**elem).rtti().finalizer {
                            fin((*elem) as *mut u8);
                        }
                        cursor.remove_current();
                        continue;
                    }
                }
                cursor.move_next();
            }
            self.current_live_mark = !self.current_live_mark;
            (*self.immix).set_current_live_mark(self.current_live_mark);
            self.los.current_live_mark = self.current_live_mark;
            self.allocated = visited;
            if visited >= self.threshold {
                self.threshold = (visited as f64 * 1.75) as usize;
            }
            #[cfg(feature = "threaded")]
            {
                safepoint::safepoint_end_gc();
            }
        }
    }

    unsafe fn collect_roots(
        &mut self,
        from: *mut *mut u8,
        to: *mut *mut u8,
        into: &mut Vec<*mut RawGc>,
    ) {
        let mut scan = from;
        let mut end = to;
        if scan > end {
            core::mem::swap(&mut scan, &mut end);
        }
        debug!("Collect roots from {:p} to {:p}", scan, end);
        while scan < end {
            let ptr = *scan;
            if ptr.is_null() {
                scan = scan.offset(1);
                continue;
            }

            if PreciseAllocation::is_precise(ptr.cast())
                && self.los.contains(Address::from_ptr(ptr))
            {
                (&mut *ptr.cast::<RawGc>()).pin();
                into.push(ptr.cast::<RawGc>());
                debug!("Found root from large object space {:p} at {:p}", ptr, scan);
                scan = scan.offset(1);
                continue;
            }

            if let Some(ptr) = (*self.immix).filter(Address::from_ptr(ptr)) {
                let ptr = ptr.to_mut_ptr::<u8>();

                (&mut *ptr.cast::<RawGc>()).pin();

                into.push(ptr.cast());
                debug!("Found root {:p} at {:p}", ptr, scan);
            }
            scan = scan.offset(1);
        }
    }

    fn allocate(&mut self, size: usize, rtti: usize) -> usize {
        unsafe {
            self.stack_end = get_sp!() as *mut u8;
            if self.allocated >= self.threshold {
                self.collect_internal(false, true);
            }
            let size = core::mem::size_of::<RawGc>() + size;

            let ptr = if size >= LARGE_OBJECT {
                self.los.alloc(size, rtti)
            } else {
                let mut addr = (*self.immix).allocate(size, 0);
                if addr.is_null() {
                    self.collect_internal(true, true);
                    addr = (*self.immix).allocate(size, 0);
                    if addr.is_null() {
                        return 0;
                    }
                }
                Address::from_ptr(addr)
            };
            let raw = &mut *ptr.to_mut_ptr::<RawGc>();
            *raw = RawGc::new(rtti);
            raw.mark(self.current_live_mark);
            if (*raw).rtti().needs_finalization && size < LARGE_OBJECT {
                #[cfg(feature = "threaded")]
                {
                    self.fin_lock.lock();
                }
                self.to_finalize.push_back(raw);
                #[cfg(feature = "threaded")]
                {
                    self.fin_lock.unlock();
                }
            }
            ptr.to_usize()
        }
    }

    fn new(size: usize, threshold: usize) -> Self {
        Self {
            allocated: 0,
            threshold,
            immix: ImmixSpace::new(align_usize(size + BLOCK_SIZE, *PAGESIZE)),
            los: LargeObjectSpace::new(),
            stack_end: 0 as *mut _,
            stack_bottom: 0 as *mut _,
            current_live_mark: false,
            collect_roots_callback: Vec::new(),

            to_finalize: LinkedList::new(),
            #[cfg(feature = "threaded")]
            fin_lock: parking_lot::RawMutex::INIT,
            collector: Collector::new(),
        }
    }
}

static mut SPACE: *mut Immix = 0 as *mut _;

/// Register callback that will be invoked when GC starts.
///
///
/// WARNING: There is no way to "unregister" this callback.
#[no_mangle]
pub extern "C" fn immix_register_ongc_callback(callback: CollectRootsCallback, data: *mut u8) {
    unsafe {
        (*SPACE).collect_roots_callback.push((callback, data));
    }
}

/// no-op callback. This is used in place of `CollectRootsCallback` internally
#[no_mangle]
pub extern "C" fn immix_noop_callback(_: *mut u8, _: TracerPtr, _: ConservativeTracer) {}
/// no-op callback for object visitor. Use this if your object does not have any pointers.
#[no_mangle]
pub extern "C" fn immix_noop_visit(_: *mut u8, _: TracerPtr) {}
/// Initialize Immix space.
///
/// ## Inputs
/// - `dummy_sp`: must be pointer to stack variable for searching roots in stack.
/// - `heap_size`: Maximum heap size. If this parameter is less than 512KB then it is set to 512KB.
/// - `threshold`: GC threshold. if zero set to 30% of `heap_size` parameter.
/// - `callback`(Optional,might be null): GC invokes this callback when collecting roots. You can use this to collect roots inside your VM.
/// - `data`(Optional,might be null): Data passed to `callback`.
#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub extern "C" fn immix_init(
    dummy_sp: *mut usize,
    mut heap_size: usize,
    mut threshold: usize,
    callback: Option<CollectRootsCallback>,
    data: *mut u8,
) {
    unsafe {
        if heap_size == 0 || heap_size <= 512 * 1024 {
            heap_size = 16 * BLOCK_SIZE;
            threshold = 100 * 1024;
        } else if threshold == 0 {
            threshold = ((30.0 * heap_size as f64) / 100.0).floor() as usize;
        }
        let mut space = Immix::new(heap_size, threshold);
        if let Some(callback) = callback {
            space.collect_roots_callback.push((callback, data));
        }
        space.stack_bottom = dummy_sp as *mut u8;

        let mem = malloc(core::mem::size_of::<Immix>()).cast::<Immix>();
        mem.write(space);
        SPACE = mem;
    }
}

/// Initialize logger library. No-op if built without `log` feature.
#[no_mangle]
pub extern "C" fn immix_init_logger() {
    #[cfg(feature = "log")]
    {
        simple_logger::SimpleLogger::new()
            .with_level(log::LevelFilter::Debug)
            .init()
            .unwrap();
    }
}
#[repr(C)]
pub struct GCObject {
    pub rtti: *const GCRTTI,
}

/// Allocate memory of `size + sizeof(GCObject)` bytes in Immix heap and set object RTTI to `rtti`. If `size` >= 8KB then
/// object is allocated inside large object space.
///
///
/// ## Return value
/// Returns pointer to allocated memory or null if allocation failed after emergency GC cycle.
///
#[no_mangle]
#[inline]
pub extern "C" fn immix_alloc(size: usize, rtti: *mut GCRTTI) -> *mut GCObject {
    unsafe { (*SPACE).allocate(size, rtti as _) as *mut GCObject }
}

pub fn immix_alloc_safe<T: HeapObject>(value: T) -> Gc<T> {
    unsafe {
        let ptr = immix_alloc(value.heap_size(), object_ty_of_type::<T>() as *mut _);
        let ptr = ptr as *mut RawGc;
        (*ptr).data().cast::<T>().write(value);
        Gc {
            marker: Default::default(),
            ptr: NonNull::new_unchecked(ptr),
        }
    }
}

/// Trigger garbage collection. If `move_objects` is true might potentially move unpinned objects.
///
///  
/// NOTE: If libimmixcons was built with `threaded` feature this function inside might wait for other
/// threads to reach yieldpoints or give up to other thread that started collection.
#[no_mangle]
#[inline]
pub extern "C" fn immix_collect(move_objects: bool) {
    unsafe {
        (*SPACE).collect_internal(move_objects, false);
    }
}
pub(crate) static PAGESIZE: once_cell::sync::Lazy<usize> = once_cell::sync::Lazy::new(|| unsafe {
    #[cfg(target_family = "windows")]
    {
        let mut si: SYSTEM_INFO = std::mem::MaybeUninit::zeroed().assume_init();
        GetSystemInfo(&mut si);
        si.dwPageSize as _
    }
    #[cfg(target_family = "unix")]
    {
        let page_size = libc::sysconf(libc::_SC_PAGESIZE);
        page_size as _
    }
});
