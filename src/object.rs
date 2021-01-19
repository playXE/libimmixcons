use crate::util::*;
use core::ops::{Deref, DerefMut};
use core::ptr::*;
use core::{i16, marker::PhantomData};
#[repr(C)]
pub struct RawGc {
    pub vtable: TaggedPointer<usize>,
}

/// Visits garbage collected objects
///
/// This should only be used by a [HeapImpl]
pub trait Tracer {
    /// Traces a reference to a specified value.
    fn trace(&mut self, reference: &mut NonNull<RawGc>);
}

#[macro_export]
macro_rules! make_rtti_for {
    ($t: ty) => {
        GCRTTI {
            visit_references: {
                extern "C" fn visit(data: *mut u8, trace: TracerPtr) {
                    unsafe {
                        (*data.add(8).cast::<$t>()).visit_references(&mut *core::mem::transmute::<
                            [usize; 2],
                            *mut dyn Tracer,
                        >(
                            trace.tracer
                        ));
                    }
                }
                visit
            },
            finalizer: Some({
                extern "C" fn fin(data: *mut u8) {
                    unsafe {
                        core::ptr::drop_in_place(data.add(8).cast::<$t>());
                    }
                }
                fin
            }),
            needs_finalization: false,
            heap_size: {
                extern "C" fn size(data: *mut u8) -> usize {
                    unsafe { (*data.add(8).cast::<$t>()).heap_size() }
                }
                size
            },
        }
    };
    (finalize $t: ty) => {
        GCRTTI {
            visit_references: {
                extern "C" fn visit(data: *mut u8, trace: TracerPtr) {
                    unsafe {
                        (*data.add(8).cast::<$t>()).visit_references(&mut *core::mem::transmute::<
                            [usize; 2],
                            *mut dyn Tracer,
                        >(
                            trace.tracer
                        ));
                    }
                }
                visit
            },
            finalizer: Some({
                extern "C" fn fin(data: *mut u8) {
                    unsafe {
                        core::ptr::drop_in_place(data.add(8).cast::<$t>());
                    }
                }
                fin
            }),
            needs_finalization: true,
            heap_size: {
                extern "C" fn size(data: *mut u8) -> usize {
                    unsafe { (*data.add(8).cast::<$t>()).heap_size() }
                }
                size
            },
        }
    };
}

/// Indicates that a type can be traced and safely allocated by a garbage collector.
///
///
/// ## Safety
/// See the documentation of the `visit_references` method for more info.
/// Essentially, this object must faithfully trace anything that
/// could contain garbage collected pointers or other `HeapObject` items.
///
/// Custom destructors must never reference garbage collected pointers.
/// The garbage collector may have already freed the other objects
/// before calling this type's drop function.
///
/// Unlike java finalizers, this allows us to deallocate objects normally
/// and avoids a second pass over the objects
/// to check for resurrected objects.
pub trait HeapObject {
    const RTTI: GCRTTI;
    /// Visit each field in this type.
    ///
    ///
    /// Users should never invoke this method.
    /// Only the collector itself is premitted to call this method,
    /// and **it is undefined behavior for the user to invoke this**.
    ///
    ///
    /// Structures should trace each of their fields,
    /// and collections should trace each of their elements.
    ///
    /// ### Safety
    /// Some types (like `Gc`) need special actions taken when they're traced,
    /// but those are easily handled: just invoke `visit_references` on `Gc`,
    /// and it will be properly passed to `tracer`.
    ///
    /// ## Always Permitted
    /// - Reading your own memory (includes iteration)
    ///   - Interior mutation is undefined behavior.
    /// - Calling `Tracer::trace` on `&mut <Gc>::ptr` field.
    ///   
    /// - Panicking
    ///   - This should be reserved for cases where you are seriously screwed up,
    ///       and can't fulfill your contract to trace your interior properly.
    /// ## Never Permitted Behavior
    /// - Forgetting a element of a collection, or field of a structure
    ///   - If you forget an element undefined behavior will result
    ///   - This is why we always prefer automatically derived implementations where possible.
    ///     - You will never trigger undefined behavior with an automatic implementation,
    ///       and it'll always be completely sufficient for safe code (aside from destructors).
    ///     - With an automatically derived implementation you will never miss a field
    /// - It is undefined behavior to mutate any of your own data.
    ///   - The mutable `&mut self` is just so copying collectors can relocate GC pointers
    /// - Invoking this function directly, without delegating to `Tracer`.
    #[allow(unused_variables)]
    fn visit_references(&mut self, tracer: &mut dyn Tracer) {
        // no-op by default
    }
    /// Returns *real* size of object on the heap. For static objects it is equal to `size_of_val(self)` but
    /// if you need some variable sized object on heap (i.e array) you have to change this function impl so
    /// it will return proper size for array (example: `self.len() * size_of::<T>() + size_of::<Array<T>>()`)
    fn heap_size(&self) -> usize {
        core::mem::size_of_val(self)
    }
    /// If this type needs a destructor run
    ///
    /// This is usually equivalent to `std::mem::needs_drop`.
    /// However procedurally derived code can sometimes provide
    /// a no-op drop implementation (for safety),
    /// which would lead to a false positive with `std::mem::needs_drop()`
    #[inline(always)]
    fn needs_finalization(&self) -> bool {
        false
    }
}
pub fn object_ty_of<T: HeapObject>(_: *const T) -> usize {
    &T::RTTI as *const GCRTTI as usize
}

pub fn object_ty_of_type<T: HeapObject + Sized>() -> usize {
    let result = object_ty_of(core::ptr::null::<T>());
    debug_assert_ne!(result, 0);
    result
}
impl RawGc {
    pub fn rtti(&self) -> &GCRTTI {
        unsafe { &*(self.vtable() as *mut GCRTTI) }
    }
    pub fn object_size(&self) -> usize {
        align_usize(
            (self.rtti().heap_size)(self as *const Self as *mut u8) + core::mem::size_of::<Self>(),
            16,
        )
    }

    pub fn data(&self) -> *mut u8 {
        unsafe { (self as *const Self as *const u8).add(core::mem::size_of::<Self>()) as *mut u8 }
    }
    /// Return true if this object is precise allocation
    pub fn is_precise_allocation(&self) -> bool {
        crate::large_object_space::PreciseAllocation::is_precise(self as *const _ as *mut _)
    }
    /// Return precise allocation from this object
    pub fn precise_allocation(&self) -> *mut crate::large_object_space::PreciseAllocation {
        crate::large_object_space::PreciseAllocation::from_cell(self as *const _ as *mut _)
    }
    pub fn new(vtable: usize) -> Self {
        Self {
            vtable: TaggedPointer::new(vtable as *mut _),
        }
    }

    pub fn mark(&mut self, mark: bool) -> bool {
        let prev = self.vtable.bit_is_set(0);

        debug_assert!(!self.vtable.untagged().is_null());
        self.vtable.set_bit_x(mark, 0);
        debug_assert!(self.vtable.bit_is_set(0) == mark);
        prev == mark
    }
    pub fn get_mark(&self) -> bool {
        debug_assert!(!self.vtable.untagged().is_null());
        self.vtable.bit_is_set(0)
    }
    pub fn pin(&mut self) {
        debug_assert!(!self.vtable.untagged().is_null());
        self.vtable.set_bit(2);
    }

    pub fn is_pinned(&self) -> bool {
        debug_assert!(!self.vtable.untagged().is_null());
        self.vtable.bit_is_set(2)
    }

    pub fn unpin(&mut self) {
        debug_assert!(!self.vtable.untagged().is_null());
        self.vtable.clear_bit(2);
    }

    pub fn is_forwarded(&self) -> bool {
        self.vtable.bit_is_set(1)
    }
    pub fn set_forwarded(&mut self, addr: usize) {
        debug_assert!(!self.vtable.untagged().is_null());
        self.vtable = TaggedPointer::new(addr as *mut _);
        self.vtable.set_bit(1);
    }

    pub fn vtable(&self) -> usize {
        debug_assert!(!self.vtable.untagged().is_null());
        self.vtable.untagged() as usize
    }
}
/// rounds the given value `val` up to the nearest multiple
/// of `align`.
pub fn align_usize(value: usize, align: usize) -> usize {
    if align == 0 {
        return value;
    }

    ((value + align - 1) / align) * align
}

/// A garbage collected pointer to a value.
///
/// This is the equivalent of a garbage collected smart-pointer.
///
/// The smart pointer is simply a guarantee to the garbage collector
/// that this points to a garbage collected object with the correct header,
/// and not some arbitrary bits that you've decided to heap allocate.
///
/// NOTE: GC is smart enough to find out that for example reference like this `&*my_gc`
/// on stack points into some object by aligning down to 16 bytes so you do not have to worry about it.
pub struct Gc<T: HeapObject + ?Sized> {
    pub ptr: NonNull<RawGc>,
    pub marker: PhantomData<T>,
}

impl<T: HeapObject> Deref for Gc<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*(&*self.ptr.as_ptr()).data().cast::<T>() }
    }
}

impl<T: HeapObject> DerefMut for Gc<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *(&*self.ptr.as_ptr()).data().cast::<T>() }
    }
}
impl<T: HeapObject + ?Sized> Gc<T> {
    pub fn from_raw(ptr: *const T) -> Self {
        Self {
            marker: PhantomData,
            ptr: unsafe { NonNull::new_unchecked(ptr.cast::<RawGc>().sub(1) as *mut RawGc) },
        }
    }
}
#[derive(Clone, Copy)]
#[repr(C)]
pub struct TracerPtr {
    pub tracer: [usize; 2],
}

impl TracerPtr {
    pub fn trace(self, val: &mut *mut RawGc) {
        unsafe {
            (*core::mem::transmute::<[usize; 2], *mut dyn Tracer>(self.tracer))
                .trace(core::mem::transmute(val));
        }
    }
}

#[no_mangle]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn tracer_trace(p: TracerPtr, gc_val: *mut *mut RawGc) {
    unsafe {
        p.trace(&mut *gc_val);
    }
}

macro_rules! impl_for_prim {
    ($($t: ident)*) => {
        $(
            impl HeapObject for $t {
                const RTTI: GCRTTI = make_rtti_for!($t);
            }
        )*
    };
}

impl_for_prim!(
    u8 i8
    u16 i16
    u32 i32
    u64 i64
    u128 i128
    f32 f64
    bool
);

/// Main type used for object tracing,finalization and allocation.
///
///
///
///
///
///
///
///
///
///
///
#[repr(C)]
pub struct GCRTTI {
    /// Returns object size on heap. Must be non null when using from c/c++!
    pub heap_size: extern "C" fn(*mut u8) -> usize,
    /// Traces object for references into GC heap. Might be null when using from c/c++.
    pub visit_references: extern "C" fn(*mut u8, TracerPtr),
    /// If set to true object that uses this RTTI will be pushed to `to_finalize` list and might be finalized at some GC cycle.
    pub needs_finalization: bool,
    /// Object finalizer. Invoked when object is dead.
    pub finalizer: Option<extern "C" fn(*mut u8)>,
}

#[repr(C)]
/// ConservativeTracer is passed into GC callback so users of this library can also provide some region of memory for conservative scan.
pub struct ConservativeTracer {
    pub(crate) roots: *mut u8,
}

impl ConservativeTracer {
    pub fn add(&self, start: *mut *mut u8, end: *mut *mut u8) {
        unsafe {
            (&mut *(self.roots as *mut alloc::vec::Vec<(usize, usize)>))
                .push((start as usize, end as usize));
        }
    }
}
/// Add memory region from `begin` to `end` for scanning for heap objects.
#[no_mangle]
pub extern "C" fn conservative_roots_add(
    tracer: *mut ConservativeTracer,
    begin: usize,
    end: usize,
) {
    unsafe { (&mut *tracer).add(begin as *mut _, end as *mut _) }
}

impl<T: HeapObject + ?Sized> Copy for Gc<T> {}
impl<T: HeapObject + ?Sized> Clone for Gc<T> {
    fn clone(&self) -> Self {
        *self
    }
}

static NOOP_SINK: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[inline]
#[no_mangle]
pub extern "C" fn immix_noop1(word: usize) {
    NOOP_SINK.store(word, core::sync::atomic::Ordering::Relaxed)
}

/// Explicitly tell the collector that an object is reachable    
/// at a particular program point.  This prevents the argument   
/// reference from being optimized away, even it is otherwise no   
/// longer needed.  It should have no visible effect in the      
/// absence of finalizers or disappearing links.  But it may be  
/// needed to prevent finalizers from running while the          
/// associated external resource is still in use.                
/// The function is sometimes called keep_alive in other         
/// settings.  
/// ```ingore
/// let x = immix_alloc_safe(Foo {...});
/// keep_on_stack!(&x); // 'x' will be on stack and not optimized by compiler to registers.
///    
///    
///          
/// ```

#[macro_export]
macro_rules! keep_on_stack {
    ($($e: expr),*) => {
        $(
            $crate::object::immix_noop1($e as *const _ as usize);
        )*
    }
}
