# libimmixcons

Implementation of Immix Mark-Region Garbage collector written in Rust Programming Language.

# Status

This is mostly usable library. You can use this library inside your programs or VM implementation safely.

- Threading support when built with `threaded` option.
- Trap based safepoints for almost zero overhead.
- Conservative stack scanning and precise on heap scanning.
- Opportunistic evacuation of fragmented blocks.
- C API in `libimmixcons.h`.

# Building

To build library for use in C/C++ or other language with C FFI use these commands:

```rust
RUSTFLAGS="-Clinker-plugin-lto" cargo build --release //  add --no-default-features to build single threaded GC
```

To link statically with your binary you should use LTO for maximum performance:

```bash
# Compile the C code with `-flto=thin`
clang -c -O2 -flto=thin -o main.o ./main.c
# Link everything, making sure that we use an appropriate linker
clang -flto=thin -fuse-ld=lld -L<path to libimmixcons.a> -llibimmixcons -o main -O2 ./cmain.o
```

# TODO

- More documentation and more examples.
- More benchmarks.
- ~~I want more power!~~ Improve performance as it is not at its peak right now.

  To win BDWGC in benchmarks we have to disable unmapping blocks after each GC cycle. To beat it in regular programs
  we have to implement chunk allocation so we can unmap chunks of blocks rathern than each block individually.

# Examples

For examples look in `examples/` directory and for usage of C API take a look at `example.c`.

# Benchmarks

gcbench results against BDWGC on iMac mid 2011:

```
     Running target/release/deps/gcbench_bdwgc-47ada9e692b50c7d
Gnuplot not found, using plotters backend
Benchmarking bdwgc: Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 77.5s, or reduce sample count to 10.
bdwgc                   time:   [718.77 ms 724.43 ms 730.26 ms]

     Running target/release/deps/gcbench_bdwgc_incremental-5f4334b96f91de18
Gnuplot not found, using plotters backend
GC Warning: Can't turn on GC incremental mode as fork() handling requested
Benchmarking bdwgc incremental: Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 74.6s, or reduce sample count to 10.
bdwgc incremental       time:   [733.45 ms 741.71 ms 750.51 ms]
Found 1 outliers among 100 measurements (1.00%)
  1 (1.00%) high mild

     Running target/release/deps/gcbench_immix-a6addca55e071dda
Gnuplot not found, using plotters backend
Benchmarking libimmixcons (30% threshold): Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 70.6s, or reduce sample count to 10.
libimmixcons (30% threshold)
                        time:   [628.04 ms 659.71 ms 694.79 ms]
Found 3 outliers among 100 measurements (3.00%)
  2 (2.00%) high mild
  1 (1.00%) high severe
```

gcbench results against BDWGC with LTO enabled on iMac mid 2011:

```
Benchmarking bdwgc: Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 79.2s, or reduce sample count to 10.
bdwgc                   time:   [786.30 ms 797.21 ms 809.01 ms]
                        change: [-10.259% -7.9337% -5.5851%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 7 outliers among 100 measurements (7.00%)
  6 (6.00%) high mild
  1 (1.00%) high severe

     Running target/release/deps/gcbench_bdwgc_incremental-9e5a3fa1d631864c
Gnuplot not found, using plotters backend
GC Warning: Can't turn on GC incremental mode as fork() handling requested
Benchmarking bdwgc incremental: Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 79.6s, or reduce sample count to 10.
bdwgc incremental       time:   [802.26 ms 817.81 ms 835.18 ms]
                        change: [+3.6590% +6.5725% +9.8956%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 10 outliers among 100 measurements (10.00%)
  7 (7.00%) high mild
  3 (3.00%) high severe

     Running target/release/deps/gcbench_immix-811b9e52fc9ac3c4
Gnuplot not found, using plotters backend
Benchmarking libimmixcons (30% threshold): Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 54.3s, or reduce sample count to 10.
libimmixcons (30% threshold)
                        time:   [441.69 ms 449.68 ms 457.32 ms]
                        change: [-0.7763% +1.6894% +4.2178%] (p = 0.18 > 0.05)
                        No change in performance detected.
Found 1 outliers among 100 measurements (1.00%)
  1 (1.00%) high mild
```
