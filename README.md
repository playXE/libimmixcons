# libimmixcons
Implementation of Immix Mark-Region Garbage collector written in Rust Programming Language.

# Status
This is mostly usable library. You can use this library inside your programs or VM implementation safely.

- Threading support when built with `threaded` option.
- Trap based safepoints for almost zero overhead.
- Conservative stack scanning and precise on heap scanning.
- Opportunistic evacuation of fragmented blocks.
- C API in `libimmixcons.h`. 

# TODO
- More documentation and more examples.
- More benchmarks.
- ~~I want more power!~~ Improve performance as it is not at its peak right now.


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