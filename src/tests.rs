use crate::{
    immix_alloc_safe, immix_collect, immix_init, immix_init_logger,
    object::Gc,
    threading::{immix_mutator_yieldpoint, immix_register_thread, immix_unregister_thread},
};

static INIT: std::sync::Once = std::sync::Once::new();

fn init() {
    INIT.call_once(|| {
        immix_init_logger();
    });
}

#[test]
fn simple() {
    init();
    let mut sp = 0;
    immix_init(&mut sp, 2 * 1024 * 1024 * 1024, 0, None, 0 as *mut _);
    immix_register_thread(&mut sp);
    immix_mutator_yieldpoint();
    inner_simple();
    immix_unregister_thread();
}

#[inline(never)]
fn inner_simple() {
    let p = immix_alloc_safe(42);
    assert_eq!(*p, 42);
    immix_collect(true);
    let x = immix_alloc_safe(3);
    println!("{:p} {:p}", &p, &x);
    assert_eq!(*x, 3);
    assert_eq!(*p, 42);
    println!("simple done");
}

#[test]
fn smash() {
    init();

    let mut sp = 0;
    immix_init(&mut sp, 2 * 1024 * 1024 * 1024, 0, None, 0 as *mut _);
    //immix_register_thread(&mut sp);
    immix_mutator_yieldpoint();
    inner_smash();
    immix_unregister_thread();
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
