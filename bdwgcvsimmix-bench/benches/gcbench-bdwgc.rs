#![allow(dead_code, non_snake_case, unused_variables, non_upper_case_globals)]
use bdwgcvsimmix_bench::*;
use criterion::{criterion_group, criterion_main, Criterion};
fn gcbench(space: &mut Heap) {
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
    let mut long_lived = space.allocate(Node {
        left: None,
        right: None,
        i: 0,
        j: 0,
    });
    Populate(kLongLivedTreeDepth, &mut long_lived, space);
    let mut d = kMinTreeDepth;
    while d <= kMaxTreeDepth {
        TimeConstruction(d, space);
        d += 2;
    }
    /*println!(
        "GC bench finished\n  GC threshold is now: {}\n GC cycles happened: {}",
        formatted_size(space.gc_threshold()),
        space.num_collections()
    );
    println!("long lived addr {:p}", &long_lived);*/
}

pub struct Node {
    left: Option<Gc<Self>>,
    right: Option<Gc<Self>>,
    i: i32,
    j: i32,
}

fn TreeSize(i: i32) -> i32 {
    (1 << (i + 1)) - 1
}

fn NumIters(i: i32) -> i32 {
    2 * TreeSize(kStretchTreeDepth) / TreeSize(i)
}
fn Populate(idepth: i32, thisnode: &mut Gc<Node>, space: &mut Heap) {
    if idepth <= 0 {
        return;
    }
    thisnode.left = Some(space.allocate(Node {
        left: None,
        right: None,
        i: 0,
        j: 0,
    }));
    thisnode.right = Some(space.allocate(Node {
        left: None,
        right: None,
        i: 0,
        j: 0,
    }));
    Populate(idepth - 1, thisnode.left.as_mut().unwrap(), space);
    Populate(idepth - 1, thisnode.right.as_mut().unwrap(), space)
}

fn MakeTree(idepth: i32, space: &mut Heap) -> Gc<Node> {
    if idepth <= 0 {
        return space.allocate(Node {
            left: None,
            right: None,
            i: 0,
            j: 0,
        });
    } else {
        let left = MakeTree(idepth - 1, space);
        let right = MakeTree(idepth - 1, space);
        let result = space.allocate(Node {
            left: Some(left),
            right: Some(right),
            i: 0,
            j: 0,
        });
        result
    }
}
#[inline(never)]
fn TimeConstruction(depth: i32, space: &mut Heap) {
    let iNumIters = NumIters(depth);

    let start = instant::Instant::now();
    for _ in 0..iNumIters {
        let mut tempTree = space.allocate(Node {
            left: None,
            right: None,
            i: 0,
            j: 0,
        });
        Populate(depth, &mut tempTree, space);

        // destroy tempTree
    }

    let start = instant::Instant::now();
    for _ in 0..iNumIters {
        let tempTree = MakeTree(depth, space);
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

fn criterion_bench(c: &mut Criterion) {
    let mut heap = Heap::new();

    c.bench_function("bdwgc", |b| b.iter(|| gcbench(&mut heap)));
    let mut group = c.benchmark_group("threaded");
    group.sample_size(10);
    /*group.bench_function("bdwgc", |b| {
        b.iter(|| {
            let mut threads = Vec::with_capacity(4);
            for _ in 0..2 {
                threads.push(std::thread::spawn(move || {
                    let mut heap = heap;
                    gcbench(&mut heap);
                }));
            }

            while let Some(th) = threads.pop() {
                th.join().unwrap();
            }
        });
    });*/
}
criterion_group!(benches, criterion_bench);
criterion_main!(benches);
