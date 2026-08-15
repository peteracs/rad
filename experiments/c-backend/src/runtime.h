#ifndef RAD_RUNTIME_H
#define RAD_RUNTIME_H

#include <stdint.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef RAD_SEPARATE_COMPILATION
#define RAD_DECL extern
#else
#define RAD_DECL static
#endif

#define RAD_MAX_COMPONENTS 1024
#define RAD_MAX_CALL_DEPTH 512

/* ========== Core Types ========== */

typedef struct {
    char *data;
    int64_t len;
} RadString;

typedef struct {
    int64_t *data;
    int64_t len;
    int64_t cap;
} RadIntList;

typedef struct {
    uint64_t *words;
    int64_t capacity;
} RadBitSet;

typedef struct {
    char *data;
    int64_t len;
    int64_t cap;
} RadBuffer;

typedef struct RadValue_ RadValue;
typedef struct RadFieldStore_ RadFieldStore;
struct RadFieldStore_ {
    RadValue *fields;
    int64_t len;
    int64_t refcount;
};
typedef struct RadStruct_ RadStruct;
struct RadStruct_ {
    RadFieldStore *store;
    int64_t layout_comp;
};
typedef struct {
    RadValue *data;
    int64_t len;
    int64_t cap;
} RadList;

typedef struct RadWorldFork_ RadWorldFork;
typedef struct RadEnv_ RadEnv;
typedef struct RadRefColumn_ RadRefColumn;
typedef struct RadRefU64Array_ RadRefU64Array;

typedef enum {
    RV_NIL = 0,
    RV_INT = 1,
    RV_FLOAT = 2,
    RV_BOOL = 3,
    RV_STR = 4,
    RV_LIST_INT = 5,
    RV_LIST = 6,
    RV_ENTITY = 7,
    RV_FN = 8,
    RV_BITSET = 9,
    RV_WORLD_FORK = 10,
    RV_BUFFER = 11,
    RV_TUPLE = 12,
    RV_MAP = 13,
    RV_STRUCT = 14,
    RV_OPTION_SOME = 15,
    RV_OPTION_NONE = 16,
    RV_RESULT_OK = 17,
    RV_RESULT_ERR = 18
} RadTag;

struct RadValue_ {
    RadTag tag;
    union {
        int64_t i;
        double f;
        bool b;
        RadString str;
        RadIntList *list_i;
        RadList *list;
        int64_t entity_id;
        struct { RadValue (*fn_ptr)(RadEnv *, RadValue *, int64_t); RadEnv *env; } fn;
        RadBitSet *bitset;
        RadWorldFork *world_fork;
        RadBuffer *buffer;
        RadList *tuple;
        struct { RadValue *keys; RadValue *vals; int64_t len; int64_t cap; int64_t *buckets; int64_t bucket_cap; } *map;
        RadStruct *rst;
        RadValue *inner;
    } as;
};

struct RadRefColumn_ {
    RadValue *data;
    int64_t capacity;
    int64_t refcount;
};

struct RadRefU64Array_ {
    uint64_t *data;
    int64_t len_words;
    int64_t refcount;
};

struct RadWorldFork_ {
    RadRefColumn **columns;
    int64_t num_components;
    RadRefU64Array *entity_masks_ref;
    RadRefU64Array *entity_alive_ref;
    int64_t mask_cap;
    int64_t mask_words;
    int64_t next_entity;
    RadValue entity_names;
    RadValue entity_id_to_name;
    bool entity_names_init;
    int64_t *free_ids;
    int64_t free_count;
    int64_t free_cap;
};

struct RadEnv_ {
    RadValue *slots;
    int64_t count;
};

typedef RadValue (*RadEventHandlerFn)(RadValue);

/* All runtime functions — declared here, defined in runtime.c */
RAD_DECL RadEnv *rad_env_new(int64_t count);
RAD_DECL RadValue rad_make_fn(RadValue (*fn_ptr)(RadEnv *, RadValue *, int64_t), RadEnv *env);
RAD_DECL RadValue rad_call(RadValue callee, RadValue *args, int64_t nargs);

RAD_DECL RadValue rad_make_nil(void);
RAD_DECL RadValue rad_make_int(int64_t x);
RAD_DECL RadValue rad_make_float(double x);
RAD_DECL RadValue rad_make_bool(bool x);
RAD_DECL RadValue rad_make_str(const char *s);
RAD_DECL const char *rad_intern_string(const char *s, int64_t len);
RAD_DECL RadValue rad_value_deep_copy(RadValue v);

RAD_DECL int64_t rad_to_int(RadValue v);
RAD_DECL bool rad_is_truthy(RadValue v);
RAD_DECL void rad_print(RadValue v);
RAD_DECL RadValue rad_print_many(RadValue *vals, int64_t n);

RAD_DECL RadValue print(RadValue v);
RAD_DECL RadValue rad_len(RadValue v);
RAD_DECL RadValue str(RadValue v);
RAD_DECL RadValue rad_assert(RadValue cond, RadValue msg);
RAD_DECL RadValue range(RadValue start, RadValue stop);
RAD_DECL RadValue rad_range_step(RadValue start, RadValue stop, RadValue step);

RAD_DECL RadValue rad_add(RadValue a, RadValue b);
RAD_DECL RadValue rad_sub(RadValue a, RadValue b);
RAD_DECL RadValue rad_mul(RadValue a, RadValue b);
RAD_DECL RadValue rad_div(RadValue a, RadValue b);
RAD_DECL RadValue rad_mod(RadValue a, RadValue b);
RAD_DECL RadValue rad_eq(RadValue a, RadValue b);
RAD_DECL RadValue rad_neq(RadValue a, RadValue b);
RAD_DECL RadValue rad_lt(RadValue a, RadValue b);
RAD_DECL RadValue rad_lte(RadValue a, RadValue b);
RAD_DECL RadValue rad_gt(RadValue a, RadValue b);
RAD_DECL RadValue rad_gte(RadValue a, RadValue b);
RAD_DECL RadValue rad_neg(RadValue a);
RAD_DECL RadValue rad_min(RadValue a, RadValue b);
RAD_DECL RadValue rad_max(RadValue a, RadValue b);

RAD_DECL RadValue rad_make_list(void);
RAD_DECL RadValue rad_list_literal(RadValue *elements, int64_t len);
RAD_DECL RadValue rad_push(RadValue lst, RadValue val);
RAD_DECL RadValue rad_index(RadValue obj, RadValue idx);
RAD_DECL RadValue rad_slice(RadValue obj, RadValue start, RadValue end);
RAD_DECL RadValue rad_sort(RadValue lst);
RAD_DECL RadValue rad_list_int_literal(const int64_t *arr, int64_t n);

RAD_DECL int64_t rad_register_component(void);
RAD_DECL void rad_register_field_name(int64_t field_id, const char *name);
RAD_DECL void rad_register_field_layout(int64_t field_id, int64_t parent_comp_id, int64_t ordinal);
RAD_DECL void rad_register_state_variant(int64_t comp_id);
RAD_DECL RadValue rad_spawn(void);
RAD_DECL RadValue rad_spawn_named(RadValue name);
RAD_DECL RadValue rad_get_entity(RadValue name);
RAD_DECL RadValue rad_despawn(RadValue ent);
RAD_DECL RadValue rad_ecs_set(RadValue ent, int64_t comp_id, RadValue val);
RAD_DECL RadValue rad_ecs_has(RadValue ent, int64_t comp_id);
RAD_DECL RadValue rad_ecs_get(RadValue ent, int64_t comp_id);
RAD_DECL RadValue rad_ecs_require(RadValue ent, int64_t comp_id);
RAD_DECL RadValue rad_ecs_remove(RadValue ent, int64_t comp_id);
RAD_DECL RadValue rad_ecs_merge(RadValue dest, RadValue src);

RAD_DECL RadValue rad_byte_len(RadValue s);
RAD_DECL RadValue rad_byte_at(RadValue s, RadValue idx);
RAD_DECL RadValue rad_substring_bytes(RadValue s, RadValue start, RadValue end);
RAD_DECL RadValue rad_try_int(RadValue s);
RAD_DECL RadValue rad_try_float(RadValue s);
RAD_DECL RadValue rad_map_or(RadValue opt, RadValue default_val, RadValue fn_unused);

RAD_DECL RadValue rad_read_file(RadValue path);
RAD_DECL RadValue rad_write_file(RadValue path, RadValue content);
RAD_DECL RadValue rad_read_file_bytes(RadValue path);
RAD_DECL RadValue rad_write_file_bytes(RadValue path, RadValue bytes);
RAD_DECL RadValue rad_append_file(RadValue path, RadValue content);
RAD_DECL RadValue rad_file_exists(RadValue path);
RAD_DECL RadValue rad_remove_file(RadValue path);
RAD_DECL RadValue rad_list_dir(RadValue path);
RAD_DECL RadValue rad_create_dir(RadValue path);
RAD_DECL RadValue rad_remove_dir(RadValue path);
RAD_DECL RadValue rad_write_stdout(RadValue v);
RAD_DECL RadValue rad_write_stderr(RadValue v);

RAD_DECL RadValue rad_bitset_new(void);
RAD_DECL RadValue rad_bitset_set(RadValue bs_val, RadValue idx_val);
RAD_DECL RadValue rad_bitset_clear(RadValue bs_val, RadValue idx_val);
RAD_DECL RadValue rad_bitset_has(RadValue bs_val, RadValue idx_val);

RAD_DECL RadValue rad_sys_args(void);
RAD_DECL RadValue rad_event_on(const char *event_name, RadEventHandlerFn handler, int once);
RAD_DECL RadValue rad_event_emit(const char *event_name, RadValue payload);
RAD_DECL RadValue rad_flush_events(void);
RAD_DECL void rad_task_context_push(void);
RAD_DECL void rad_task_context_pop(void);
RAD_DECL RadValue rad_task_from_value(RadValue v);
RAD_DECL RadValue rad_await_task(RadValue task);

RAD_DECL RadValue rad_buffer_new(void);
RAD_DECL RadValue rad_buffer_append(RadValue buf_val, RadValue str_val);
RAD_DECL RadValue rad_buffer_to_str(RadValue buf_val);

RAD_DECL RadValue rad_fork(void);
#ifdef RAD_RELEASE
#define rad_debug_trace(x) (x)
#else
RAD_DECL RadValue rad_debug_trace(RadValue v);
#endif
RAD_DECL RadValue rad_gc_collect(void);
RAD_DECL void rad_dispatch_system(RadValue sys_name);
RAD_DECL RadValue rad_simulate(RadValue fork_val, RadValue systems, RadValue ticks);
RAD_DECL RadValue rad_commit(RadValue fork_val);
RAD_DECL RadValue rad_peek(RadValue fork_val, RadValue ent, int64_t comp_id);
RAD_DECL RadValue rad_clock(void);

RAD_DECL RadValue rad_pop(RadValue lst);
RAD_DECL RadValue rad_pop_last(RadValue lst);
RAD_DECL RadValue rad_drop_last(RadValue lst);
RAD_DECL RadValue rad_reverse(RadValue lst);
RAD_DECL RadValue rad_append(RadValue a, RadValue b);
RAD_DECL RadValue rad_zip(RadValue a, RadValue b);
RAD_DECL RadValue rad_enumerate(RadValue list);

RAD_DECL RadValue rad_typeof(RadValue v);
RAD_DECL RadValue rad_unwrap(RadValue v);
RAD_DECL RadValue rad_unwrap_or(RadValue v, RadValue def);
RAD_DECL RadValue rad_is_some(RadValue v);
RAD_DECL RadValue rad_is_none(RadValue v);
RAD_DECL RadValue rad_expect(RadValue v, RadValue msg);

RAD_DECL RadValue rad_chr(RadValue v);
RAD_DECL RadValue rad_ord(RadValue v);
RAD_DECL RadValue rad_split(RadValue s, RadValue sep);
RAD_DECL RadValue rad_join(RadValue lst, RadValue sep);
RAD_DECL RadValue rad_chars(RadValue s);
RAD_DECL RadValue rad_trim(RadValue s);
RAD_DECL RadValue rad_starts_with(RadValue s, RadValue prefix);
RAD_DECL RadValue rad_ends_with(RadValue s, RadValue suffix);
RAD_DECL RadValue rad_to_upper(RadValue s);
RAD_DECL RadValue rad_to_lower(RadValue s);
RAD_DECL RadValue rad_string_repeat(RadValue s, RadValue n);
RAD_DECL RadValue rad_contains(RadValue s, RadValue sub);
RAD_DECL RadValue rad_replace(RadValue s, RadValue old_s, RadValue new_s);

RAD_DECL RadValue rad_to_int_val(RadValue v);
RAD_DECL RadValue rad_to_float(RadValue v);
RAD_DECL RadValue rad_int_div(RadValue a, RadValue b);

RAD_DECL RadValue rad_eprint(RadValue v);
RAD_DECL RadValue rad_eprint_many(RadValue *vals, int64_t n);
RAD_DECL RadValue rad_flush(void);

RAD_DECL RadValue rad_hof_map(RadValue lst, RadValue fn);
RAD_DECL RadValue rad_hof_filter(RadValue lst, RadValue fn);
RAD_DECL RadValue rad_hof_reduce(RadValue lst, RadValue init, RadValue fn);
RAD_DECL RadValue rad_hof_sort_by(RadValue lst, RadValue fn);
RAD_DECL RadValue rad_hof_flat_map(RadValue lst, RadValue fn);
RAD_DECL RadValue rad_hof_find(RadValue lst, RadValue fn);
RAD_DECL RadValue rad_hof_max_by(RadValue lst, RadValue fn);
RAD_DECL RadValue rad_hof_min_by(RadValue lst, RadValue fn);
RAD_DECL RadValue rad_format(RadValue fmt, RadValue *vals, int64_t nvals);
RAD_DECL RadValue rad_entries(RadValue m);
RAD_DECL RadValue rad_merge(RadValue a, RadValue b);
RAD_DECL RadValue rad_group_by(RadValue lst, RadValue fn);
RAD_DECL void rad_register_transition(int64_t from_comp, const char *event_name, int64_t to_comp, int guard_true);
RAD_DECL RadValue rad_transition(RadValue state, RadValue event_name);

RAD_DECL RadValue rad_rand_seed(RadValue v);
RAD_DECL RadValue rad_rand_int(RadValue lo, RadValue hi);
RAD_DECL RadValue rad_rand_float(void);
RAD_DECL RadValue rad_rand_bool(void);

RAD_DECL RadValue rad_index_set(RadValue obj, RadValue idx, RadValue val);
RAD_DECL RadValue rad_abs(RadValue v);

RAD_DECL RadValue rad_make_tuple(RadValue *elems, int64_t n);
RAD_DECL RadValue rad_tuple_get(RadValue tup, int64_t idx);
RAD_DECL int64_t  rad_tuple_len(RadValue tup);

RAD_DECL RadValue rad_make_struct_literal(RadValue *fields, int64_t n, int64_t layout_comp);
RAD_DECL RadValue rad_value_get_comp_field(RadValue v, int64_t idx, int64_t field_comp_id);
RAD_DECL RadValue rad_value_comp_field_set(RadValue obj, int64_t idx, int64_t field_comp_id, RadValue val);
RAD_DECL void rad_value_mut_set_field(RadValue obj, int64_t idx, int64_t field_comp_id, RadValue val);

RAD_DECL RadValue rad_make_map(void);
RAD_DECL RadValue rad_map_set(RadValue m, RadValue key, RadValue val);
RAD_DECL RadValue rad_map_get(RadValue m, RadValue key);
RAD_DECL RadValue rad_map_keys(RadValue m);
RAD_DECL RadValue rad_keys(RadValue obj);
RAD_DECL RadValue rad_map_values(RadValue m);
RAD_DECL int64_t  rad_map_len(RadValue m);
RAD_DECL RadValue rad_map_contains(RadValue m, RadValue key);
RAD_DECL RadValue rad_map_remove(RadValue m, RadValue key);
RAD_DECL RadValue rad_map_literal(RadValue *keys, RadValue *vals, int64_t n);

RAD_DECL RadValue rad_query(int64_t *comp_ids, int64_t n_comps);

RAD_DECL RadValue rad_format_value(RadValue val, RadValue spec);

RAD_DECL int g_argc;
RAD_DECL char **g_argv;
RAD_DECL int g_rad_call_depth;

#ifdef RAD_DEBUG_ARENA
RAD_DECL void rad_debug_init(void);
#endif

#endif
