#[link(name = "gc")]
extern "C" {
    fn GC_malloc(_: usize) -> usize;
    fn GC_init();
}

fn main() {
    unsafe {
        GC_init();
        let p = GC_malloc(8);
        println!("{:x}", p);
    }
    println!("Hello, world!");
}
