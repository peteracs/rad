/* Manual reproduction (no Rad emit): shows C runtime + emitter-style patterns.

   Build (from repo root):
     gcc -O2 -DRAD_REPRO_METRICS -I core/c-backend/src core/c-backend/repro/senior_review_manual_repro.c -o core/c-backend/target/repro_senior/manual.exe -lm

   Effect: prints REPRO_NEXT_ENTITY ... delta >> 0 from repeated
   rad_ecs_set(ent, comp, ({ __sv = rad_spawn(); ...; __sv }))
   which matches emit_struct_var_as_value + rad_ecs_set(deep_copy).
*/
#include <stdint.h>
#include <stdio.h>
#include <string.h>

/* Define RAD_REPRO_METRICS on the compiler command line. */
#include "runtime.c"

static int64_t __comp_Pos;
static int64_t __comp_Option_None;
static int64_t __comp_Option_Some;
static int64_t __comp_Result_Ok;
static int64_t __comp_Result_Err;
static int64_t __comp_None;
static int64_t __comp_Some;
static int64_t __comp_Ok;
static int64_t __comp_Err;
static int64_t __field_x;
static int64_t __field_value;
static int64_t __field_message;

/* Same as emitted `rad_variant_of`; required when linking runtime.c alone. */
RadValue rad_variant_of(RadValue v) {
    if (v.tag != RV_ENTITY) return rad_make_nil();
    if (rad_is_truthy(rad_ecs_has(v, __comp_Option_None))) return rad_make_str("None");
    if (rad_is_truthy(rad_ecs_has(v, __comp_Option_Some))) return rad_make_str("Some");
    if (rad_is_truthy(rad_ecs_has(v, __comp_Result_Ok))) return rad_make_str("Ok");
    if (rad_is_truthy(rad_ecs_has(v, __comp_Result_Err))) return rad_make_str("Err");
    return rad_make_nil();
}

typedef struct {
    RadValue x;
} RadComp_Pos;

static RadValue make_pos_entity(RadValue x) {
    return ({
        RadValue __sl_ent = rad_spawn();
        rad_ecs_set(__sl_ent, __field_x, x);
        __sl_ent;
    });
}

/* Same shape as emit_c `emit_struct_var_as_value` for Pos + local struct p. */
static RadValue struct_var_as_value_pos(RadComp_Pos p) {
    return ({
        RadValue __sv = rad_spawn();
        rad_ecs_set(__sv, __field_x, p.x);
        __sv;
    });
}

int main(int argc, char **argv) {
    g_argc = argc;
    g_argv = argv;

    __comp_Pos = rad_register_component();
    __comp_Option_None = rad_register_component();
    __comp_Option_Some = rad_register_component();
    __comp_Result_Ok = rad_register_component();
    __comp_Result_Err = rad_register_component();
    __comp_None = rad_register_component();
    __comp_Some = rad_register_component();
    __comp_Ok = rad_register_component();
    __comp_Err = rad_register_component();
    __comp_None = __comp_Option_None;
    __comp_Some = __comp_Option_Some;
    __comp_Ok = __comp_Result_Ok;
    __comp_Err = __comp_Result_Err;
    __field_x = rad_register_component();
    rad_register_field_name(__field_x, "x");
    __field_value = rad_register_component();
    rad_register_field_name(__field_value, "value");
    __field_message = rad_register_component();
    rad_register_field_name(__field_message, "message");

    RadValue ent = rad_spawn();
    rad_ecs_set(ent, __comp_Pos, make_pos_entity(rad_make_int(1)));

    int64_t before = rad_repro_metrics_next_entity();
    const int N = 1000;
    for (int i = 0; i < N; i++) {
        RadValue cv = rad_ecs_require(ent, __comp_Pos);
        RadComp_Pos p = ({
            (RadComp_Pos){.x = rad_ecs_require(cv, __field_x)};
        });
        /* Emitter path for `set(ent, p)` when p is typed Pos */
        rad_ecs_set(ent, __comp_Pos, struct_var_as_value_pos(p));
    }
    int64_t after = rad_repro_metrics_next_entity();
    fprintf(stderr,
            "REPRO_NEXT_ENTITY before=%lld after=%lld delta=%lld (loop=%d)\n",
            (long long)before, (long long)after, (long long)(after - before), N);

    /* --- Point 3: same RV_ENTITY tag, different deep_copy behavior --- */
    RadValue plain = rad_spawn();
    rad_ecs_set(plain, __comp_Pos, make_pos_entity(rad_make_int(42)));
    RadValue plain_copy = rad_value_deep_copy(plain);
    fprintf(stderr, "REPRO_HEURISTIC plain_eid=%lld copy_eid=%lld (expect same id; world entity)\n",
            (long long)plain.as.entity_id, (long long)plain_copy.as.entity_id);

    RadValue marked = rad_spawn();
    rad_ecs_set(marked, __comp_Pos, make_pos_entity(rad_make_int(42)));
    rad_ecs_set(marked, __comp_Option_Some, rad_make_bool(true));
    rad_ecs_set(marked, __field_value, rad_make_int(99));
    RadValue marked_copy = rad_value_deep_copy(marked);
    fprintf(stderr, "REPRO_HEURISTIC marked_eid=%lld copy_eid=%lld (expect different; value-like)\n",
            (long long)marked.as.entity_id, (long long)marked_copy.as.entity_id);

    return 0;
}
