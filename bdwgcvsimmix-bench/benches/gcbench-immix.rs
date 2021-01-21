#![allow(dead_code, non_snake_case, unused_variables, non_upper_case_globals)]
use criterion::{criterion_group, criterion_main, Criterion};
use libimmixcons::{object::*, *};
use threading::{immix_mutator_yieldpoint, immix_register_thread};
pub struct Node {
    left: Option<Gc<Self>>,
    right: Option<Gc<Self>>,
    i: i32,
    j: i32,
}
impl HeapObject for Node {
    const RTTI: GCRTTI = make_rtti_for!(Node);
    fn visit_references(&mut self, tracer: &mut dyn Tracer) {
        match self.left {
            Some(ref mut left) => left.visit_references(tracer),
            _ => (),
        }
        match self.right {
            Some(ref mut right) => right.visit_references(tracer),
            _ => (),
        }
    }
}
fn TreeSize(i: i32) -> i32 {
    (1 << (i + 1)) - 1
}

fn NumIters(i: i32) -> i32 {
    2 * TreeSize(kStretchTreeDepth) / TreeSize(i)
}
#[inline(never)]
fn Populate(idepth: i32, mut thisnode: Gc<Node>) {
    if idepth <= 0 {
        return;
    }
    keep_on_stack!(&mut thisnode);
    thisnode.left = Some(immix_alloc_safe(Node {
        left: None,
        right: None,
        i: 0,
        j: 0,
    }));
    thisnode.right = Some(immix_alloc_safe(Node {
        left: None,
        right: None,
        i: 0,
        j: 0,
    }));
    immix_mutator_yieldpoint();
    Populate(idepth - 1, thisnode.left.unwrap());
    Populate(idepth - 1, thisnode.right.unwrap())
}
#[inline(never)]
fn MakeTree(idepth: i32) -> Gc<Node> {
    immix_mutator_yieldpoint();
    if idepth <= 0 {
        return immix_alloc_safe(Node {
            left: None,
            right: None,
            i: 0,
            j: 0,
        });
    } else {
        let left = MakeTree(idepth - 1);
        let right = MakeTree(idepth - 1);
        let result = immix_alloc_safe(Node {
            left: Some(left),
            right: Some(right),
            i: 0,
            j: 0,
        });
        result
    }
}

static mut FOO: *mut Gc<Node> = 0 as *mut _;
#[inline(never)]
fn TimeConstruction(depth: i32) {
    let iNumIters = NumIters(depth);

    for _ in 0..iNumIters {
        let mut tempTree = immix_alloc_safe(Node {
            left: None,
            right: None,
            i: 0,
            j: 0,
        });

        Populate(depth, tempTree);
        unsafe {
            FOO = &mut tempTree;
        }
        // destroy tempTree
    }

    for _ in 0..iNumIters {
        let tempTree = MakeTree(depth);
    }
}
const kStretchTreeDepth: i32 = 18;
const kLongLivedTreeDepth: i32 = 16;
const kArraySize: i32 = 500000;
const kMinTreeDepth: i32 = 4;
const kMaxTreeDepth: i32 = 16;
struct Array {
    value: [f64; kArraySize as usize],
}
#[inline(never)]
fn gcbench() {
    /*simple_logger::SimpleLogger::new()
    .with_level(log::LevelFilter::Debug)
    .init();*/

    /*println!(
        " Live storage will peak at {}.\n",
        formatted_size(
            (2 * (size_of::<Node>() as i32) * TreeSize(kLongLivedTreeDepth)
                + (size_of::<Array>() as i32)) as usize
        )
    );*/

    /*  println!(
        " Stretching memory with a binary tree or depth {}",
        kStretchTreeDepth
    );*/
    let mut long_lived = immix_alloc_safe(Node {
        left: None,
        right: None,
        i: 0,
        j: 0,
    });

    Populate(kLongLivedTreeDepth, long_lived);
    let mut d = kMinTreeDepth;
    while d <= kMaxTreeDepth {
        TimeConstruction(d);
        d += 2;
        immix_mutator_yieldpoint();
    }
    keep_on_stack!(&mut long_lived);
    /*println!(
        "GC bench finished\n  GC threshold is now: {}\n GC cycles happened: {}",
        formatted_size(space.gc_threshold()),
        space.num_collections()
    );
    println!("long lived addr {:p}", &long_lived);*/
}

fn criterion_bench(c: &mut Criterion) {
    immix_init(50 * 1024 * 1024, 0, immix_noop_callback, 0 as *mut _);
    //immix_enable_stats(GcStats::Summary);
    immix_register_thread();
    let mut group = c.benchmark_group("immix");
    group.sample_size(10).bench_function(
        "gcbench",
        #[inline(never)]
        |b| b.iter(|| gcbench()),
    );
    immix_dump_summary();
}
criterion_group!(benches, criterion_bench);
criterion_main!(benches);
