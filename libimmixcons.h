#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

#define BLOCK_SIZE (32 * 1024)

#define LINE_SIZE 128

#define NUM_LINES_PER_BLOCK (BLOCK_SIZE / LINE_SIZE)

/**
 * `NormalAllocator`, otherwise the `OverflowAllocator` is used.
 */
#define MEDIUM_OBJECT LINE_SIZE

/**
 * Objects larger than LARGE_OBJECT are allocated using the `LargeObjectSpace`.
 */
#define LARGE_OBJECT (8 * 1024)

/**
 * Whether evacuation should be used or not.
 */
#define USE_EVACUATION true

/**
 * The number of blocks stored into the `EvacAllocator` for evacuation.
 */
#define EVAC_HEADROOM 5

/**
 * Ratio when to trigger evacuation collection.
 */
#define EVAC_TRIGGER_THRESHHOLD 0.25

#if defined(IMMIX_THREADED)
#define GC_STATE_WAITING 1
#endif

#if defined(IMMIX_THREADED)
#define GC_STATE_SAFE 2
#endif

typedef struct TracerPtr {
  uintptr_t tracer[2];
} TracerPtr;

typedef void (*CollectRootsCallback)(uint8_t *data, struct TracerPtr tracer);

/**
 * Main type used for object tracing,finalization and allocation.
 *
 *
 *
 *
 *
 *
 *
 *
 *
 *
 *
 */
typedef struct GCRTTI {
  /**
   * Returns object size on heap. Must be non null when using from c/c++!
   */
  uintptr_t (*heap_size)(uint8_t*);
  /**
   * Traces object for references into GC heap. Might be null when using from c/c++.
   */
  void (*visit_references)(uint8_t*, struct TracerPtr);
  /**
   * If set to true object that uses this RTTI will be pushed to `to_finalize` list and might be finalized at some GC cycle.
   */
  bool needs_finalization;
  /**
   * Object finalizer. Invoked when object is dead.
   */
  void (*finalizer)(uint8_t*);
} GCRTTI;

typedef struct GCObject {
  const struct GCRTTI *rtti;
} GCObject;

/**
 * Structure wrapping a raw, tagged pointer.
 */
typedef struct TaggedPointer_usize {
  uint64_t raw;
} TaggedPointer_usize;

typedef struct RawGc {
  struct TaggedPointer_usize vtable;
} RawGc;

/**
 * no-op callback. This is used in place of `CollectRootsCallback` internally
 */
void immix_noop_callback(uint8_t*, struct TracerPtr);

/**
 * no-op callback for object visitor. Use this if your object does not have any pointers.
 */
void immix_noop_visit(uint8_t*, struct TracerPtr);

/**
 * Initialize Immix space.
 *
 * ## Inputs
 * - `dummy_sp`: must be pointer to stack variable for searching roots in stack.
 * - `heap_size`: Maximum heap size. If this parameter is less than 512KB then it is set to 512KB.
 * - `threshold`: GC threshold. if zero set to 30% of `heap_size` parameter.
 * - `callback`: GC invokes this callback when collecting roots. You can use this to collect roots inside your VM.
 * - `data`: Data passed to `callback`.
 */
void immix_init(uintptr_t *dummy_sp,
                uintptr_t heap_size,
                uintptr_t threshold,
                CollectRootsCallback callback,
                uint8_t *data);

/**
 * Initialize logger library. No-op if built without `log` feature.
 */
void immix_init_logger(void);

/**
 * Allocate memory of `size + sizeof(GCObject)` bytes in Immix heap and set object RTTI to `rtti`. If `size` >= 8KB then
 * object is allocated inside large object space.
 *
 *
 * ## Return value
 * Returns pointer to allocated memory or null if allocation failed after emergency GC cycle.
 *
 */
struct GCObject *immix_alloc(uintptr_t size,
                             struct GCRTTI *rtti);

/**
 * Trigger garbage collection. If `move_objects` is true might potentially move unpinned objects.
 *
 *
 * NOTE: If libimmixcons was built with `threaded` feature this function inside might wait for other
 * threads to reach yieldpoints or give up to other thread that started collection.
 */
void immix_collect(bool move_objects);

void tracer_trace(struct TracerPtr p, struct RawGc **gc_val);

#if defined(IMMIX_THREADED)
void immix_prepare_thread(uintptr_t *sp);
#endif

#if defined(IMMIX_THREADED)
/**
 * Checks if current thread should yield. GC won't be able to stop a mutator unless this function is put into code.
 *
 * # Performance overhead
 * This function is no-op when libimmixcons was built without `threaded` feature. When `threaded` feature is enabled
 * this will emit volatile load without any conditional jumps so it is very small overhead compared to conditional yieldpoints.
 */
void immix_mutator_yieldpoint(void);
#endif

#if defined(IMMIX_THREADED)
/**
 * Registers main thread.
 *
 * # Panics
 * Panics if main thread is already registered.
 *
 *
 */
void immix_register_main_thread(uint8_t *dummy_sp);
#endif

#if defined(IMMIX_THREADED)
/**
 * Register thread.
 * ## Inputs
 * `sp`: pointer to variable on stack for searching roots on stack.
 *
 */
void immix_register_thread(uintptr_t *sp);
#endif

#if defined(IMMIX_THREADED)
/**
 * Unregister thread.
 */
void immix_unregister_thread(void);
#endif

#if !defined(IMMIX_THREADED)
/**
 * Registers main thread.
 *
 * # Panics
 * Panics if main thread is already registered.
 *
 *
 */
void immix_register_main_thread(void);
#endif

#if !defined(IMMIX_THREADED)
/**
 * Register thread.
 * ## Inputs
 * `sp`: pointer to variable on stack for searching roots on stack.
 *
 */
void immix_register_thread(void);
#endif

#if !defined(IMMIX_THREADED)
/**
 * Unregister thread.
 */
void immix_unregister_thread(void);
#endif

#if !defined(IMMIX_THREADED)
/**
 * Checks if current thread should yield. GC won't be able to stop a thread unless this function is put into code.
 *
 * # Performance overhead
 * This function is no-op when libimmixcons was built without `threaded` feature. When `threaded` feature is enabled
 * this will emit volatile load without any conditional jumps so it is very small overhead compared to conditional yieldpoints.
 */
void immix_mutator_yieldpoint(void);
#endif
