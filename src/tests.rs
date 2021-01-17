use crate::{
    immix_alloc, immix_alloc_safe, immix_collect, immix_init, immix_init_logger,
    immix_noop_callback,
    object::*,
    threading::{immix_mutator_yieldpoint, immix_register_thread},
};

static INIT: std::sync::Once = std::sync::Once::new();

fn init() {
    INIT.call_once(|| {
        immix_init_logger();
        immix_init(2 * 1024 * 1024 * 1024, 0, immix_noop_callback, 0 as *mut _);
        immix_register_thread();
    });
}

#[test]
fn simple() {
    init();

    inner_simple();
}

#[inline(never)]
fn inner_simple() {
    let p = immix_alloc_safe(42);
    assert_eq!(*p, 42);
    immix_collect(true);
    let x = immix_alloc_safe(3);
    // println!("{:p} {:p}", &p, &x);
    assert_eq!(*x, 3);
    assert_eq!(*p, 42);
    //println!("simple done");
}

#[test]
fn smash() {
    init();
    inner_smash();
}

#[inline(never)]
fn inner_smash() {
    let mut arr: [Option<Gc<i32>>; 7000] = [None; 7000];
    for i in 0..7000 {
        arr[i] = Some(immix_alloc_safe(4));
        immix_mutator_yieldpoint();
        if i % 3000 == 0 {
            immix_collect(true);
        }
        if i % 5678 == 0 {
            assert_eq!(*arr[i / 2000].unwrap(), 4);
            **arr[i / 2000].as_mut().unwrap() = 42;
        }
    }
    assert!(true);
}

static DUMMY_RTTI2048: GCRTTI = GCRTTI {
    needs_finalization: false,
    heap_size: {
        extern "C" fn s(_: *mut u8) -> usize {
            2048
        }
        s
    },
    visit_references: {
        extern "C" fn s(_: *mut u8, _: TracerPtr) {}
        s
    },
    finalizer: None,
};
static DUMMY_RTTI4096: GCRTTI = GCRTTI {
    needs_finalization: false,
    heap_size: {
        extern "C" fn s(_: *mut u8) -> usize {
            2048 * 2
        }
        s
    },
    visit_references: {
        extern "C" fn s(_: *mut u8, _: TracerPtr) {}
        s
    },
    finalizer: None,
};
#[test]
fn middle() {
    init();
    for _ in 0..20000 {
        immix_alloc(4096, &DUMMY_RTTI4096);
        immix_alloc(4096, &DUMMY_RTTI4096);
    }

    for _ in 0..20000 {
        immix_alloc(2048, &DUMMY_RTTI2048);
        immix_alloc(2048, &DUMMY_RTTI2048);
    }
    assert!(true);
}
