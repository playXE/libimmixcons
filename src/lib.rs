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
#![cfg_attr(test, feature(const_in_array_repeat_expressions))]
#![cfg_attr(not(test), no_std)]

#[cfg(feature = "log")]
#[macro_use]
extern crate log;

use core::ptr::NonNull;

use allocation::ImmixSpace;
use constants::{BLOCK_SIZE, LARGE_OBJECT};
use core::sync::atomic::Ordering;
use large_object_space::LargeObjectSpace;
extern crate alloc;

#[no_mangle]
pub extern "C" fn immix_enable_stats(val: GcStats) {
    unsafe {
        (*SPACE).gc_stats = val;
    }
}
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(i32)]
pub enum GcStats {
    None = 0,
    Summary,
    Verbose,
}
#[cfg(unix)]
extern "C" {
    fn printf(c: *const i8, ...) -> i32;
}
#[no_mangle]
pub extern "C" fn immix_dump_summary() {
    unsafe {
        let stats = &(*SPACE).stats;
        let runtime = (*SPACE).timer.stop();
        let (mutator, gc) = stats.percentage(runtime);
        #[cfg(unix)]
        printf(
            b"GC stats: total=%.1f\n\0".as_ptr().cast(),
            runtime as libc::c_double,
        );
        #[cfg(unix)]
        printf(
            b"GC stats: mutator=%.1f\n\0".as_ptr().cast(),
            stats.mutator(runtime) as libc::c_double,
        );
        #[cfg(unix)]
        printf(
            b"GC stats: collection=%.1f\n\n\0".as_ptr().cast(),
            stats.pause() as libc::c_double,
        );

        #[cfg(unix)]
        printf(
            b"GC stats: collections count=%i\n\0".as_ptr().cast(),
            stats.collections() as i32,
        );
        #[cfg(unix)]printf(b"GC summary: %.1fms collection (%i), %.1fms mutator, %.1f total (%f%% mutator, %f%% GC)\n\0".as_ptr().cast(),stats.pause() as libc::c_double,stats.collections() as i32,stats.mutator(runtime) as libc::c_double,runtime as libc::c_double,mutator as libc::c_double,gc as libc::c_double);
    }
}

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
pub mod stack_bounds;
pub mod threading;
use alloc::collections::LinkedList;
use alloc::vec::Vec;
use collector::Collector;
use large_object_space::PreciseAllocation;
use libc::malloc;
#[cfg(feature = "threaded")]
use locks::mutex::Mutex;
use object::*;
use object::{RawGc, TracerPtr};
#[cfg(feature = "threaded")]
use parking_lot::lock_api::RawMutex;

#[cfg(not(feature = "threaded"))]
use stack_bounds::StackBounds;

#[cfg(feature = "threaded")]
use threading::{immix_get_tls_state, GC_STATE_WAITING};
use util::*;

#[cfg(test)]
mod tests;

pub struct Immix {
    #[cfg(not(feature = "threaded"))]
    bounds: StackBounds,
    los: LargeObjectSpace,
    immix: *mut ImmixSpace,
    stack_bottom: *mut u8,
    gc_stats: GcStats,
    stack_end: *mut u8,
    allocated: usize,
    threshold: usize,
    current_live_mark: bool,
    collect_roots_callback: Vec<(CollectRootsCallback, *mut u8)>,
    timer: util::timer::Timer,
    collector: Collector,
    to_finalize: LinkedList<*mut RawGc>,
    #[cfg(feature = "threaded")]
    fin_lock: Mutex,
    stats: CollectionStats,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CollectionType {
    ImmixCollection,
    ImmixEvacCollection,
}

#[inline(never)]
fn stack_pointer() -> usize {
    let sp = 0usize;
    &sp as *const usize as usize
}

pub type CollectRootsCallback =
    extern "C" fn(data: *mut u8, tracer: TracerPtr, cons_tracer: ConservativeTracer);

impl Immix {
    #[allow(unused_variables)]
    #[inline(never)]
    fn collect_internal(&mut self, evacuation: bool, emergency: bool) {
        unsafe {
            let mut timer = util::timer::Timer::new(self.gc_stats != GcStats::None);
            crate::util::save_regs();
            let old_state;
            let threads;
            let stop_threads;
            #[cfg(feature = "threaded")]
            {
                let start = time::Instant::now();
                let ptls = immix_get_tls_state();
                ptls.stack_end = get_sp!() as *mut _;
                old_state = ptls.gc_state;
                ptls.atomic_gc_state()
                    .store(GC_STATE_WAITING, Ordering::Release);
                if !safepoint::safepoint_start_gc() {
                    ptls.gc_state_set(old_state, GC_STATE_WAITING);
                    return;
                }
                threads = safepoint::safepoint_wait_for_the_world();
                stop_threads = start.elapsed();
            };
            #[cfg(not(feature = "threaded"))]
            {
                stop_threads = ();
                old_state = 0;
                threads = ();
            }
            let collect_roots = time::Instant::now();
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
                    ConservativeTracer {
                        roots: &mut cons as *mut Vec<(usize, usize)> as *mut u8,
                    },
                );
                //assert!(cons.is_empty());
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
            let collect_roots = collect_roots.elapsed();
            self.collector.extend_all_blocks(all_blocks);
            let collection_type = self.collector.prepare_collection(
                evacuation,
                true,
                (*(*self.immix).block_allocator).available_blocks(),
                (*self.immix).evac_headroom(),
                (*(*self.immix).block_allocator).total_blocks(),
                emergency,
            );
            let mark = time::Instant::now();

            let visited = self.collector.collect(
                &collection_type,
                &roots,
                &precise_roots,
                &mut *self.immix,
                &mut self.los,
                !self.current_live_mark,
            );
            let mark = mark.elapsed();
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

            let prev = self.allocated;
            self.allocated = visited;
            if visited >= self.threshold {
                self.threshold = (visited as f64 * 1.75) as usize;
            }
            if self.gc_stats != GcStats::None {
                let duration = timer.stop();
                self.stats.add(duration);
                if self.gc_stats == GcStats::Verbose {
                    #[cfg(unix)]
                    printf("--GC cycle stats--\n\0".as_bytes().as_ptr().cast());
                    #[cfg(unix)]
                    printf(
                        b"GC freed %i bytes, heap %.3fKiB->%.3fKiB\n\0"
                            .as_ptr()
                            .cast(),
                        prev - visited,
                        prev as f64 / 1024f64,
                        visited as f64 / 1024f64,
                    );
                    /*#[cfg(feature = "threaded")]
                    printf!(
                        "GC suspended threads in %ims (%lns)\n\0",
                        stop_threads.whole_milliseconds() as i32,
                        stop_threads.whole_nanoseconds() as u64
                    );*/
                    #[cfg(all(unix, feature = "threaded"))]
                    printf(
                        b"GC suspended threads in %i ms (%lu ns)\n\0"
                            .as_ptr()
                            .cast(),
                        stop_threads.whole_milliseconds() as i32,
                        stop_threads.whole_nanoseconds() as u64,
                    );
                    #[cfg(unix)]
                    printf(
                        b"Collected roots in %i ms (%lu ns)\n\0".as_ptr().cast(),
                        collect_roots.whole_milliseconds() as u32,
                        collect_roots.whole_nanoseconds() as u64,
                    );
                    #[cfg(unix)]
                    printf(
                        b"Marking took %i ms (%lu ns)\n\0".as_ptr().cast(),
                        mark.whole_milliseconds() as u32,
                        mark.whole_nanoseconds() as u64,
                    );
                    #[cfg(unix)]
                    printf(
                        "Whole GC cycle took %.6f ms\n\0".as_ptr().cast(),
                        duration as libc::c_double,
                    );
                }
            }
            #[cfg(feature = "threaded")]
            {
                safepoint::safepoint_end_gc(&*threads);
                drop(threads);
                immix_get_tls_state().gc_state_set(old_state, GC_STATE_WAITING);
            }
        }
    }

    unsafe fn collect_roots(
        &mut self,
        from: *mut *mut u8,
        to: *mut *mut u8,
        into: &mut Vec<*mut RawGc>,
    ) {
        let mut scan = align_usize(from as usize, 16) as *mut *mut u8;
        let mut end = align_usize(to as usize, 16) as *mut *mut u8;
        if scan.is_null() || end.is_null() {
            return;
        }
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
            pub fn align_down(addr: usize, align: usize) -> usize {
                /*if !align.is_power_of_two() {
                    panic!("align should be power of two");
                }*/
                addr & !(align - 1)
            }
            //let ptr = align_down(ptr as usize, 16) as *mut u8;
            if let Some(ptr) = (*self.immix).filter(Address::from_ptr(ptr)) {
                let ptr = ptr.to_mut_ptr::<u8>();

                (&mut *ptr.cast::<RawGc>()).pin();

                into.push(ptr.cast());
                debug!("Found root {:p} at {:p}", ptr, scan);
            }
            let ptr = ptr.sub(8);
            if let Some(ptr) = (*self.immix).filter(Address::from_ptr(ptr)) {
                let ptr = ptr.to_mut_ptr::<u8>();

                (&mut *ptr.cast::<RawGc>()).pin();

                into.push(ptr.cast());
                debug!("Found root {:p} at {:p}", ptr, scan);
            }
            scan = scan.offset(1);
        }
    }
    #[inline]
    #[allow(unused_unsafe)]
    fn allocate(&mut self, size: usize, rtti: usize) -> usize {
        unsafe {
            self.stack_end = get_sp!() as *mut u8;
            if self.allocated >= self.threshold {
                //panic!();
                self.collect_internal(false, true);
            }
            let size = align_usize(size, 16);

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
            #[cfg(feature = "threaded")]
            {
                as_atomic!(&self.allocated;AtomicUsize)
                    .fetch_add(size, core::sync::atomic::Ordering::AcqRel);
            }
            #[cfg(not(feature = "threaded"))]
            {
                self.allocated += size;
            }
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
            timer: util::timer::Timer::new(false),
            gc_stats: GcStats::None,
            stats: CollectionStats::new(),
            #[cfg(not(feature = "threaded"))]
            bounds: StackBounds {
                origin: 0 as *mut u8,
                bound: 0 as *mut u8,
            },
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
            fin_lock: Mutex::new(),
            collector: Collector::new(),
        }
    }
}
#[cfg_attr(not(feature = "threaded"), thread_local)]
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
/// - `heap_size`: Maximum heap size. If this parameter is less than 512KB then it is set to 512KB.
/// - `threshold`: GC threshold. if zero set to 30% of `heap_size` parameter.
/// - `callback`(Optional,might be null): GC invokes this callback when collecting roots. You can use this to collect roots inside your VM.
/// - `data`(Optional,might be null): Data passed to `callback`.
#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub extern "C" fn immix_init(
    mut heap_size: usize,
    mut threshold: usize,
    callback: CollectRootsCallback,
    data: *mut u8,
) {
    unsafe {
        use core::sync::atomic::*;
        #[cfg_attr(not(feature = "threaded"), thread_local)]
        static INIT: AtomicBool = AtomicBool::new(false);
        if INIT.compare_exchange_weak(false, true, Ordering::SeqCst, Ordering::Relaxed) == Ok(false)
        {
            if heap_size == 0 || heap_size <= 512 * 1024 {
                heap_size = 16 * BLOCK_SIZE;
                threshold = 100 * 1024;
            } else if threshold == 0 {
                threshold = ((30.0 * heap_size as f64) / 100.0).floor() as usize;
            }
            let mut space = Immix::new(heap_size, threshold);

            space.collect_roots_callback.push((callback, data));
            #[cfg(not(feature = "threaded"))]
            {
                space.bounds = StackBounds::current_thread_stack_bounds();
                space.stack_bottom = space.bounds.origin as *mut _;
            }
            signals::install_default_signal_handlers();
            #[cfg(feature = "threaded")]
            {
                safepoint::safepoint_init();
            }
            let mem = malloc(core::mem::size_of::<Immix>()).cast::<Immix>();
            mem.write(space);
            SPACE = mem;
            (*SPACE).timer = util::timer::Timer::new(true);
        }
    }
}

/// Initialize logger library. No-op if built without `log` feature.
#[no_mangle]
pub extern "C" fn immix_init_logger() {
    #[cfg(feature = "log")]
    unsafe {
        static mut INIT: bool = false;
        if !INIT {
            INIT = true;
            simple_logger::SimpleLogger::new()
                .with_level(log::LevelFilter::Debug)
                .init()
                .unwrap();
        }
    }
}
#[repr(C)]
pub struct GCObject {
    pub rtti: u64,
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
pub extern "C" fn immix_alloc(size: usize, rtti: *const GCRTTI) -> *mut GCObject {
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
        use winapi::um::sysinfoapi::GetSystemInfo;
        use winapi::um::sysinfoapi::SYSTEM_INFO;
        let mut si: SYSTEM_INFO = core::mem::MaybeUninit::zeroed().assume_init();
        GetSystemInfo(&mut si);
        si.dwPageSize as _
    }
    #[cfg(target_family = "unix")]
    {
        let page_size = libc::sysconf(libc::_SC_PAGESIZE);
        page_size as _
    }
});

pub(crate) fn thread_self() -> u64 {
    #[cfg(windows)]
    unsafe {
        extern "C" {
            fn GetCurrentThreadId() -> u32;
        }
        GetCurrentThreadId() as u64
    }
    #[cfg(unix)]
    unsafe {
        libc::pthread_self() as u64
    }
}

struct CollectionStats {
    collections: usize,
    total_pause: f32,
    pauses: Vec<f32>,
}

impl CollectionStats {
    fn new() -> CollectionStats {
        CollectionStats {
            collections: 0,
            total_pause: 0f32,
            pauses: Vec::new(),
        }
    }

    fn add(&mut self, pause: f32) {
        self.collections += 1;
        self.total_pause += pause;
        self.pauses.push(pause);
    }

    fn pause(&self) -> f32 {
        self.total_pause
    }

    fn pauses(&self) -> AllNumbers {
        AllNumbers(self.pauses.clone())
    }

    fn mutator(&self, runtime: f32) -> f32 {
        runtime - self.total_pause
    }

    fn collections(&self) -> usize {
        self.collections
    }

    fn percentage(&self, runtime: f32) -> (f32, f32) {
        let gc_percentage = ((self.total_pause / runtime) * 100.0).round();
        let mutator_percentage = 100.0 - gc_percentage;

        (mutator_percentage, gc_percentage)
    }
}

pub struct AllNumbers(Vec<f32>);

impl core::fmt::Display for AllNumbers {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "[")?;
        let mut first = true;
        for num in &self.0 {
            if !first {
                write!(f, ",")?;
            }
            write!(f, "{:.1}", num)?;
            first = false;
        }
        write!(f, "]")
    }
}
