use libimmixcons::*;
use object::*;
use threading::immix_register_main_thread;
struct Simple {
    x: Gc<i32>,
}

impl HeapObject for Simple {
    // remove `finalize` if you do not have to invoke destructor for object.
    const RTTI: GCRTTI = make_rtti_for!(finalize Simple);
    fn visit_references(&mut self, tracer: &mut dyn Tracer) {
        println!("Tracing 'Simple'");
        self.x.visit_references(tracer);
    }
}

impl Drop for Simple {
    fn drop(&mut self) {
        println!("Drop for 'Simple' invoked after GC");
    }
}

fn main() {
    immix_init_logger();
    let mut sp = 0;
    immix_init(&mut sp, 0, 0, Some(immix_noop_callback), 0 as *mut _);
    immix_register_main_thread(&mut sp as *mut usize as *mut u8);
    {
        let p = immix_alloc_safe(42);
        let _s = immix_alloc_safe(Simple { x: p });
        immix_collect(true);
    }
}
