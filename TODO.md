# TODOs for libimmixcons
- More benchmarks
    - Compare against V8,JSC,SpiderMonkey and OpenJDK GCs
    - Compare against Rust GC crates (rust-gc,broom,shredder etc)
    - Add more benchmarks against BDWGC
- Extend documentation
    - Add examples on how to use this library.

- ~~I want more power!~~ Even more performance! 
    - Replace `alloc::vec::Vec` with intrusive linked lists.

            
        Right now on each GC cycle we collect blocks from each thread and allocators into different Vecs and then collect all these vectors into single one which forces quite a lot of allocations and slow downs GC. We could avoid that by embedding linked list header into block header (`next` and `prev` pointers) inside block and linking block to list.
    - Sweep on demand i.e lazy sweep.
        - Concurrent sweeping when `threaded` feature is enabled.
        - Lock-free queue for requesting new blocks. (We use Mutex on Vec right now).
        - Allocate blocks in chunks.
    - Improve performance on Windows
            

        libimmixcons currently is slow on Windows due to decomitting each block individually rather than giving hint to kernel that we do not need memory (madvise + MADV_DONTNEED on POSIX).


        NOTE: Try to use DiscardVirtualMemory instead of VirtualFree + MEM_DECOMMIT
    - Parallel marking support.


        Quite hard feature as evacuation is hard with parallel marking because we might move the same object in two marker threads.

        NOTE: Maybe we can enable parallel marking only when non evac collection is requested?
    