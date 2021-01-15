// building:
// cbindgen --config cbindgen.toml --crate libimmixcons --output libimmixcons.h
// clang -L"target/release/" -llibimmixcons example.c
//
#define IMMIX_THREADED 1
#include "libimmixcons.h"
#include <stdio.h>
#define make_simple_rtti(type)                                               \
    uintptr_t gcrtti_##type##_size(uint8_t *_unused)                         \
    {                                                                        \
        return sizeof(type);                                                 \
    }                                                                        \
    GCRTTI gcrtti_##type = {&gcrtti_##type##_size, &immix_noop_visit, 0, 0}; \
    typedef struct GC##type##_                                               \
    {                                                                        \
        const GCRTTI *rtti;                                                  \
        type value;                                                          \
    } GC##type;

void inner_main();
int main()
{
    immix_init_logger();
    void *sp = (void *)0;
    immix_init((uintptr_t *)(void *)&sp, 0, 0, &immix_noop_callback, 0);
    immix_register_thread((uintptr_t *)&sp);
    inner_main();
}
make_simple_rtti(int);
typedef struct
{
    GCRTTI *rtti;
    GCint *myInt;
} Foo;
uintptr_t fooSize(uint8_t *_unused)
{
    return 8;
}
void visit_foo(uint8_t *self_, TracerPtr tracer)
{
    Foo *self = (Foo *)self_;
    printf("trace %i\n", self->myInt->value);
    tracer_trace(tracer, (struct RawGc **)&self->myInt);
}
GCRTTI dummyRTTI = {&fooSize, &visit_foo, 0, 0};

void inner_main()
{
    GCint *obj = immix_alloc(4, &gcrtti_int);

    obj->value = 42;
    Foo *foo = immix_alloc(8, &dummyRTTI);
    foo->myInt = obj;
    immix_collect(false);
    printf("%p\n", &foo);
}