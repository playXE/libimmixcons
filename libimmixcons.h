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

typedef struct Option_CollectRootsCallback Option_CollectRootsCallback;

typedef struct TracerPtr {
  uintptr_t tracer[2];
} TracerPtr;

/**
 * ConservativeTracer is passed into GC callback so users of this library can also provide some region of memory for conservative scan.
 */
typedef struct ConservativeTracer {
  uint8_t *roots;
} ConservativeTracer;

typedef void (*CollectRootsCallback)(uint8_t *data, struct TracerPtr tracer, struct ConservativeTracer cons_tracer);

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
 * Register callback that will be invoked when GC starts.
 *
 *
 * WARNING: There is no way to "unregister" this callback.
 */
void immix_register_ongc_callback(CollectRootsCallback callback, uint8_t *data);

/**
 * no-op callback. This is used in place of `CollectRootsCallback` internally
 */
void immix_noop_callback(uint8_t*, struct TracerPtr, struct ConservativeTracer);

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
 * - `callback`(Optional,might be null): GC invokes this callback when collecting roots. You can use this to collect roots inside your VM.
 * - `data`(Optional,might be null): Data passed to `callback`.
 */
void immix_init(uintptr_t *dummy_sp,
                uintptr_t heap_size,
                uintptr_t threshold,
                struct Option_CollectRootsCallback callback,
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

/**
 * Add memory region from `begin` to `end` for scanning for heap objects.
 */
void conservative_roots_add(struct ConservativeTracer *tracer, uintptr_t begin, uintptr_t end);

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

#if defined(IMMIX_THREADED)
/**
 * Enter unsafe GC state. This means current thread runs "managed by GC code" and GC *must* stop this thread
 * at GC cycle.
 *
 * Returns current state to restore later.
 */
int8_t immix_unsafe_enter(void);
#endif

#if defined(IMMIX_THREADED)
/**
 * Leave unsafe GC state and restore previous state from `state` argument. This function has yieldpoint internally so thread
 * might be suspended for GC.
 */
int8_t immix_unsafe_leave(int8_t state);
#endif

#if defined(IMMIX_THREADED)
/**
 * Enter safe for GC state. When thread is in safe state it is allowed to execute code at the same time with the GC.
 *
 *
 * Returns current state to restore later.
 */
int8_t immix_safe_enter(void);
#endif

#if defined(IMMIX_THREADED)
/**
 * Leave safe for GC state and restore previous state from `state` argument.
 */
int8_t immix_safe_leave(int8_t state);
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
void immix_register_main_thread(uint8_t*);
#endif

#if !defined(IMMIX_THREADED)
/**
 * Register thread.
 * ## Inputs
 * `sp`: pointer to variable on stack for searching roots on stack.
 *
 */
void immix_register_thread(uintptr_t*);
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

#if !defined(IMMIX_THREADED)
/**
 * Enter unsafe GC state. This means current thread runs "managed by GC code" and GC *must* stop this thread
 * at GC cycle.
 *
 * Returns current state to restore later.
 */
int8_t immix_unsafe_enter(void);
#endif

#if !defined(IMMIX_THREADED)
/**
 * Leave unsafe GC state and restore previous state from `state` argument. This function has yieldpoint internally so thread
 * might be suspended for GC.
 */
int8_t immix_unsafe_leave(int8_t state);
#endif

#if !defined(IMMIX_THREADED)
/**
 * Enter safe for GC state. When thread is in safe state it is allowed to execute code at the same time with the GC.
 *
 *
 * Returns current state to restore later.
 */
int8_t immix_safe_enter(void);
#endif

#if !defined(IMMIX_THREADED)
/**
 * Leave safe for GC state and restore previous state from `state` argument.
 */
int8_t immix_safe_leave(int8_t state);
#endif
