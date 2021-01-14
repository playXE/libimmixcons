# libimmixcons
Implementation of Immix Mark-Region Garbage collector written in Rust Programming Language.

# Status
This is mostly usable library. You can use this library inside your programs or VM implementation safely.

- Threading support when built with `threaded` option.
- Trap based safepoints for almost zero overhead.
- Conservative stack scanning and precise on heap scanning.
- Opportunistic evacuation of fragmented blocks.
- C API in `libimmixcons.h`. 

# Examples
For examples look in `examples/` directory and for usage of C API take a look at `example.c`.