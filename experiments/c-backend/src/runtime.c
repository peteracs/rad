#ifndef RAD_RUNTIME_C
#define RAD_RUNTIME_C

#include "runtime.h"
#include <dirent.h>
#include <math.h>
#include <sys/stat.h>
#ifdef _WIN32
#include <direct.h>
#endif

#ifdef RAD_SEPARATE_COMPILATION
#define RAD_API
#else
#define RAD_API static
#endif

/* ========== Arena Allocator ========== */

#define RAD_ARENA_CHUNK_SIZE (1 << 20)

typedef struct RadArenaChunk_ {
    struct RadArenaChunk_ *next;
    size_t size;
    size_t used;
    char data[];
} RadArenaChunk;

RAD_API RadArenaChunk *g_arena_head = NULL;

/* ---- Scratch Arena (scoped temporaries) ---- */
#ifdef RAD_SCRATCH_ARENA

static RadArenaChunk *g_scratch_head = NULL;
static bool g_use_scratch = false;

typedef struct {
    RadArenaChunk *chunk;
    size_t used;
} RadArenaSave;

static void *rad_scratch_alloc(size_t size) {
    size = (size + 7) & ~(size_t)7;
    RadArenaChunk *chunk = g_scratch_head;
    if (chunk && chunk->used + size <= chunk->size) {
        void *ptr = chunk->data + chunk->used;
        chunk->used += size;
        memset(ptr, 0, size);
        return ptr;
    }
    size_t chunk_size = size > RAD_ARENA_CHUNK_SIZE ? size : RAD_ARENA_CHUNK_SIZE;
    RadArenaChunk *nc = (RadArenaChunk *)malloc(sizeof(RadArenaChunk) + chunk_size);
    if (!nc) { fprintf(stderr, "rad runtime: scratch out of memory\n"); exit(1); }
    nc->next = g_scratch_head;
    nc->size = chunk_size;
    nc->used = size;
    g_scratch_head = nc;
    memset(nc->data, 0, size);
    return nc->data;
}

static RadArenaSave rad_scratch_save(void) {
    RadArenaSave s;
    s.chunk = g_scratch_head;
    s.used = g_scratch_head ? g_scratch_head->used : 0;
    g_use_scratch = true;
    return s;
}

static void rad_scratch_restore(RadArenaSave saved) {
    while (g_scratch_head && g_scratch_head != saved.chunk) {
        RadArenaChunk *old = g_scratch_head;
        g_scratch_head = old->next;
        free(old);
    }
    if (g_scratch_head) {
        g_scratch_head->used = saved.used;
    }
    g_use_scratch = (g_scratch_head != NULL);
}

static RadValue rad_scratch_promote(RadValue v);

#endif /* RAD_SCRATCH_ARENA */

/* ---- Debug Arena: canary-guarded allocations ---- */

#ifdef RAD_DEBUG_ARENA

#define RAD_CANARY_HEAD 0xDEADBEEFCAFEBABEULL
#define RAD_CANARY_TAIL 0xFEEDFACEC0FFEE42ULL

typedef struct RadAllocRecord_ {
    struct RadAllocRecord_ *next;
    void *user_ptr;
    size_t user_size;
    const char *tag;
} RadAllocRecord;

RAD_API RadAllocRecord *g_alloc_records = NULL;
RAD_API int64_t g_alloc_count = 0;
RAD_API int g_arena_validated = 0;

RAD_API void rad_debug_register(void *user_ptr, size_t user_size, const char *tag) {
    RadAllocRecord *rec = (RadAllocRecord *)malloc(sizeof(RadAllocRecord));
    if (!rec) return;
    rec->user_ptr = user_ptr;
    rec->user_size = user_size;
    rec->tag = tag;
    rec->next = g_alloc_records;
    g_alloc_records = rec;
    g_alloc_count++;
}

RAD_API int rad_debug_verify_one(RadAllocRecord *rec) {
    uint64_t *head = (uint64_t *)((char *)rec->user_ptr - 8);
    uint64_t *tail = (uint64_t *)((char *)rec->user_ptr + rec->user_size);
    int ok = 1;
    if (*head != RAD_CANARY_HEAD) {
        fprintf(stderr, "[DEBUG_ARENA] HEAD canary corrupted at %p (size=%zu, tag=%s)\n",
                rec->user_ptr, rec->user_size, rec->tag ? rec->tag : "?");
        fprintf(stderr, "  expected 0x%llx, got 0x%llx\n",
                (unsigned long long)RAD_CANARY_HEAD, (unsigned long long)*head);
        ok = 0;
    }
    if (*tail != RAD_CANARY_TAIL) {
        fprintf(stderr, "[DEBUG_ARENA] TAIL canary corrupted at %p (size=%zu, tag=%s)\n",
                rec->user_ptr, rec->user_size, rec->tag ? rec->tag : "?");
        fprintf(stderr, "  expected 0x%llx, got 0x%llx\n",
                (unsigned long long)RAD_CANARY_TAIL, (unsigned long long)*tail);
        ok = 0;
    }
    return ok;
}

RAD_API void rad_debug_validate_arena(void) {
    if (g_arena_validated) return;
    g_arena_validated = 1;
    int64_t checked = 0, corrupt = 0;
    RadAllocRecord *rec = g_alloc_records;
    while (rec) {
        if (!rad_debug_verify_one(rec)) corrupt++;
        checked++;
        RadAllocRecord *next = rec->next;
        free(rec);
        rec = next;
    }
    g_alloc_records = NULL;
    if (corrupt > 0) {
        fprintf(stderr, "[DEBUG_ARENA] CORRUPTION DETECTED: %lld of %lld allocations\n",
                (long long)corrupt, (long long)checked);
        fflush(stderr);
    } else {
        fprintf(stderr, "[DEBUG_ARENA] OK: %lld allocations verified, 0 corruptions\n",
                (long long)checked);
        fflush(stderr);
    }
}

RAD_API void *rad_arena_alloc_debug(size_t size, const char *tag) {
    size_t aligned = (size + 7) & ~(size_t)7;
    size_t total = 8 + aligned + 8;
    RadArenaChunk *chunk = g_arena_head;
    void *raw;
    if (chunk && chunk->used + total <= chunk->size) {
        raw = chunk->data + chunk->used;
        chunk->used += total;
    } else {
        size_t chunk_size = total > RAD_ARENA_CHUNK_SIZE ? total : RAD_ARENA_CHUNK_SIZE;
        RadArenaChunk *nc = (RadArenaChunk *)malloc(sizeof(RadArenaChunk) + chunk_size);
        if (!nc) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
        nc->next = g_arena_head;
        nc->size = chunk_size;
        nc->used = total;
        g_arena_head = nc;
        raw = nc->data;
    }
    *(uint64_t *)raw = RAD_CANARY_HEAD;
    void *user = (char *)raw + 8;
    memset(user, 0, aligned);
    *(uint64_t *)((char *)user + aligned) = RAD_CANARY_TAIL;
    rad_debug_register(user, aligned, tag);
    return user;
}

#define rad_arena_alloc(sz) rad_arena_alloc_debug((sz), __func__)

#endif /* RAD_DEBUG_ARENA */

#ifndef RAD_DEBUG_ARENA
RAD_API void *rad_arena_alloc(size_t size) {
    size = (size + 7) & ~(size_t)7;
    RadArenaChunk *chunk = g_arena_head;
    if (chunk && chunk->used + size <= chunk->size) {
        void *ptr = chunk->data + chunk->used;
        chunk->used += size;
        memset(ptr, 0, size);
        return ptr;
    }
    size_t chunk_size = size > RAD_ARENA_CHUNK_SIZE ? size : RAD_ARENA_CHUNK_SIZE;
    RadArenaChunk *nc = (RadArenaChunk *)malloc(sizeof(RadArenaChunk) + chunk_size);
    if (!nc) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
    nc->next = g_arena_head;
    nc->size = chunk_size;
    nc->used = size;
    g_arena_head = nc;
    memset(nc->data, 0, size);
    return nc->data;
}
#endif

RAD_API void *rad_arena_realloc(void *old, size_t old_size, size_t new_size) {
    if (new_size <= old_size) return old;
    void *p = rad_arena_alloc(new_size);
    if (old && old_size > 0) memcpy(p, old, old_size);
    return p;
}

#ifdef RAD_DEBUG_ARENA
__attribute__((constructor))
RAD_API void rad_debug_init(void) {
    atexit(rad_debug_validate_arena);
}
#endif

/* Types are in runtime.h */

RAD_API RadEnv *rad_env_new(int64_t count) {
    RadEnv *env = (RadEnv *)rad_arena_alloc(sizeof(RadEnv));
    env->count = count;
    env->slots = (RadValue *)rad_arena_alloc((size_t)count * sizeof(RadValue));
    return env;
}

RAD_API RadValue rad_make_fn(RadValue (*fn_ptr)(RadEnv *, RadValue *, int64_t), RadEnv *env) {
    RadValue v;
    v.tag = RV_FN;
    v.as.fn.fn_ptr = fn_ptr;
    v.as.fn.env = env;
    return v;
}

RAD_API RadValue rad_call(RadValue callee, RadValue *args, int64_t nargs) {
    if (callee.tag != RV_FN || !callee.as.fn.fn_ptr) {
        fprintf(stderr, "rad runtime: attempted to call non-function\n");
        exit(1);
    }
    return callee.as.fn.fn_ptr(callee.as.fn.env, args, nargs);
}

RAD_API int g_argc = 0;
RAD_API char **g_argv = NULL;
RAD_API int g_rad_call_depth = 0;

RAD_API RadValue rad_sys_args(void);
RAD_API RadValue rad_flush_events(void);

/* ========== String Interning (arena-backed) ========== */

typedef struct RadInternNode_ {
    char *str;
    int64_t len;
    struct RadInternNode_ *next;
} RadInternNode;

#define RAD_INTERN_TABLE_SIZE 16384
RAD_API RadInternNode *rad_intern_table[RAD_INTERN_TABLE_SIZE] = {NULL};

RAD_API uint32_t rad_hash_string(const char *s, int64_t len) {
    uint32_t hash = 2166136261u;
    for (int64_t i = 0; i < len; i++) {
        hash ^= (uint8_t)s[i];
        hash *= 16777619;
    }
    return hash;
}

RAD_API const char *rad_intern_string(const char *s, int64_t len) {
    if (!s) return NULL;
    uint32_t hash = rad_hash_string(s, len);
    uint32_t idx = hash % RAD_INTERN_TABLE_SIZE;
    RadInternNode *node = rad_intern_table[idx];
    while (node) {
        if (node->len == len && memcmp(node->str, s, (size_t)len) == 0) {
            return node->str;
        }
        node = node->next;
    }
    RadInternNode *new_node = (RadInternNode *)rad_arena_alloc(sizeof(RadInternNode));
    new_node->str = (char *)rad_arena_alloc((size_t)(len + 1));
    memcpy(new_node->str, s, (size_t)len);
    new_node->str[len] = '\0';
    new_node->len = len;
    new_node->next = rad_intern_table[idx];
    rad_intern_table[idx] = new_node;
    return new_node->str;
}

RAD_API RadString rad_string_copy(const char *s) {
    RadString r;
    if (!s) { r.data = NULL; r.len = 0; return r; }
    size_t n = strlen(s);
    r.data = (char *)rad_intern_string(s, (int64_t)n);
    r.len = (int64_t)n;
    return r;
}

/* ========== Value Constructors ========== */

RAD_API RadValue rad_make_nil(void) {
    RadValue v; v.tag = RV_NIL; v.as.i = 0; return v;
}
RAD_API RadValue rad_make_int(int64_t x) {
    RadValue v; v.tag = RV_INT; v.as.i = x; return v;
}
RAD_API RadValue rad_make_float(double x) {
    RadValue v; v.tag = RV_FLOAT; v.as.f = x; return v;
}
RAD_API RadValue rad_make_bool(bool x) {
    RadValue v; v.tag = RV_BOOL; v.as.b = x; return v;
}
RAD_API RadValue rad_make_str(const char *s) {
    RadValue v; v.tag = RV_STR; v.as.str = rad_string_copy(s); return v;
}

/* ========== Int Lists (arena-backed) ========== */

RAD_API RadIntList *rad_list_int_new(void) {
    RadIntList *xs = (RadIntList *)rad_arena_alloc(sizeof(RadIntList));
    xs->data = NULL; xs->len = 0; xs->cap = 0;
    return xs;
}

RAD_API void rad_list_int_reserve(RadIntList *xs, int64_t need) {
    if (!xs || xs->cap >= need) return;
    int64_t cap = xs->cap == 0 ? 8 : xs->cap;
    while (cap < need) cap *= 2;
    xs->data = (int64_t *)rad_arena_realloc(xs->data,
        (size_t)xs->cap * sizeof(int64_t), (size_t)cap * sizeof(int64_t));
    xs->cap = cap;
}

RAD_API void rad_list_int_push(RadIntList *xs, int64_t v) {
    if (!xs) return;
    rad_list_int_reserve(xs, xs->len + 1);
    xs->data[xs->len++] = v;
}

RAD_API int64_t rad_list_int_get(RadIntList *xs, int64_t i) {
    if (!xs || i < 0 || i >= xs->len) return 0;
    return xs->data[i];
}

RAD_API RadValue rad_list_int_literal(const int64_t *arr, int64_t n) {
    RadIntList *xs = rad_list_int_new();
    if (n > 0) {
        rad_list_int_reserve(xs, n);
        memcpy(xs->data, arr, (size_t)n * sizeof(int64_t));
        xs->len = n;
    }
    RadValue v; v.tag = RV_LIST_INT; v.as.list_i = xs;
    return v;
}

/* ========== Helpers ========== */

RAD_API int64_t rad_to_int(RadValue v) {
    switch (v.tag) {
    case RV_INT: return v.as.i;
    case RV_BOOL: return v.as.b ? 1 : 0;
    case RV_FLOAT: return (int64_t)v.as.f;
    default: return 0;
    }
}

RAD_API RadIntList *rad_to_list(RadValue v) {
    if (v.tag == RV_LIST_INT) return v.as.list_i;
    return NULL;
}

RAD_API inline bool rad_is_alive(int64_t eid);
RAD_API RadValue rad_variant_of(RadValue v);
static RadValue rad_value_deep_copy_from_fork(RadValue v, const RadWorldFork *fork);
#ifdef RAD_SEPARATE_COMPILATION
extern int64_t __comp_Option_None;
extern int64_t __comp_Option_Some;
extern int64_t __comp_Result_Ok;
extern int64_t __comp_Result_Err;
extern int64_t __field_value;
extern int64_t __field_message;
#else
static int64_t __comp_Option_None;
static int64_t __comp_Option_Some;
static int64_t __comp_Result_Ok;
static int64_t __comp_Result_Err;
static int64_t __field_value;
static int64_t __field_message;
#endif
RAD_API int64_t rad_next_component_id;
static const char *rad_field_names[RAD_MAX_COMPONENTS];
static unsigned char rad_state_variant_ids[RAD_MAX_COMPONENTS];

RAD_API bool rad_is_truthy(RadValue v) {
    switch (v.tag) {
    case RV_NIL: return false;
    case RV_BOOL: return v.as.b;
    case RV_INT: return v.as.i != 0;
    case RV_FLOAT: return v.as.f != 0.0;
    case RV_STR: return v.as.str.len != 0;
    case RV_LIST_INT: return v.as.list_i && v.as.list_i->len != 0;
    case RV_LIST: return v.as.list && v.as.list->len != 0;
    case RV_ENTITY: return rad_is_alive(v.as.entity_id);
    case RV_FN: return v.as.fn.fn_ptr != NULL;
    case RV_BITSET: return v.as.bitset != NULL;
    case RV_WORLD_FORK: return v.as.world_fork != NULL;
    case RV_STRUCT: return v.as.rst != NULL;
    default: return false;
    }
}

/* ========== String Concat (arena temp buffer) ========== */

RAD_API RadValue rad_str_concat(RadValue a, RadValue b) {
    const char *sa = (a.tag == RV_STR && a.as.str.data) ? a.as.str.data : "";
    const char *sb = (b.tag == RV_STR && b.as.str.data) ? b.as.str.data : "";
    int64_t la = (a.tag == RV_STR) ? a.as.str.len : (int64_t)strlen(sa);
    int64_t lb = (b.tag == RV_STR) ? b.as.str.len : (int64_t)strlen(sb);

    char stack_buf[256];
    char *temp;
    size_t need = (size_t)(la + lb + 1);
    if (need <= sizeof(stack_buf)) {
        temp = stack_buf;
    } else {
        temp = (char *)malloc(need);
        if (!temp) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
    }
    memcpy(temp, sa, (size_t)la);
    memcpy(temp + la, sb, (size_t)lb);
    temp[la + lb] = '\0';

    RadValue v;
    v.tag = RV_STR;
    v.as.str.data = (char *)rad_intern_string(temp, la + lb);
    v.as.str.len = la + lb;
    if (need > sizeof(stack_buf)) free(temp);
    return v;
}

/* ========== Arithmetic / Comparison Ops ========== */

RAD_API RadValue rad_add(RadValue a, RadValue b) {
    if (a.tag == RV_STR || b.tag == RV_STR) return rad_str_concat(a, b);
    if (a.tag == RV_FLOAT || b.tag == RV_FLOAT) {
        double fa = (a.tag == RV_FLOAT) ? a.as.f : (double)rad_to_int(a);
        double fb = (b.tag == RV_FLOAT) ? b.as.f : (double)rad_to_int(b);
        return rad_make_float(fa + fb);
    }
    return rad_make_int(rad_to_int(a) + rad_to_int(b));
}
RAD_API RadValue rad_sub(RadValue a, RadValue b) {
    if (a.tag == RV_FLOAT || b.tag == RV_FLOAT) {
        double fa = (a.tag == RV_FLOAT) ? a.as.f : (double)rad_to_int(a);
        double fb = (b.tag == RV_FLOAT) ? b.as.f : (double)rad_to_int(b);
        return rad_make_float(fa - fb);
    }
    return rad_make_int(rad_to_int(a) - rad_to_int(b));
}
RAD_API RadValue rad_mul(RadValue a, RadValue b) {
    if (a.tag == RV_STR && (b.tag == RV_INT || b.tag == RV_FLOAT))
        return rad_string_repeat(a, b);
    if (b.tag == RV_STR && (a.tag == RV_INT || a.tag == RV_FLOAT))
        return rad_string_repeat(b, a);
    if (a.tag == RV_FLOAT || b.tag == RV_FLOAT) {
        double fa = (a.tag == RV_FLOAT) ? a.as.f : (double)rad_to_int(a);
        double fb = (b.tag == RV_FLOAT) ? b.as.f : (double)rad_to_int(b);
        return rad_make_float(fa * fb);
    }
    return rad_make_int(rad_to_int(a) * rad_to_int(b));
}
RAD_API RadValue rad_div(RadValue a, RadValue b) {
    if (a.tag == RV_FLOAT || b.tag == RV_FLOAT) {
        double fa = (a.tag == RV_FLOAT) ? a.as.f : (double)rad_to_int(a);
        double fb = (b.tag == RV_FLOAT) ? b.as.f : (double)rad_to_int(b);
        if (fb == 0.0) { fprintf(stderr, "rad runtime: division by zero\n"); exit(1); }
        return rad_make_float(fa / fb);
    }
    int64_t ib = rad_to_int(b);
    if (ib == 0) { fprintf(stderr, "rad runtime: division by zero\n"); exit(1); }
    return rad_make_int(rad_to_int(a) / ib);
}
RAD_API RadValue rad_mod(RadValue a, RadValue b) {
    int64_t ib = rad_to_int(b);
    if (ib == 0) { fprintf(stderr, "rad runtime: modulo by zero\n"); exit(1); }
    return rad_make_int(rad_to_int(a) % ib);
}

RAD_API int rad_str_eq(RadString a, RadString b) {
    if (a.data == b.data) return 1;
    if (a.len != b.len) return 0;
    if (!a.data || !b.data) return 0;
    return memcmp(a.data, b.data, (size_t)a.len) == 0;
}

RAD_API RadValue rad_eq(RadValue a, RadValue b) {
    if (a.tag == RV_STR && b.tag == RV_STR) return rad_make_bool(rad_str_eq(a.as.str, b.as.str));
    if (a.tag == RV_NIL && b.tag == RV_NIL) return rad_make_bool(true);
    if (a.tag == RV_NIL || b.tag == RV_NIL) return rad_make_bool(false);
    if (a.tag == RV_BOOL && b.tag == RV_BOOL) return rad_make_bool(a.as.b == b.as.b);
    if (a.tag == RV_ENTITY && b.tag == RV_ENTITY) {
        if (a.as.entity_id == b.as.entity_id) return rad_make_bool(true);
        RadValue va = rad_variant_of(a);
        RadValue vb = rad_variant_of(b);
        if (va.tag == RV_STR && vb.tag == RV_STR && rad_str_eq(va.as.str, vb.as.str)) {
            for (int64_t comp_id = 0; comp_id < rad_next_component_id && comp_id < RAD_MAX_COMPONENTS; comp_id++) {
                if (!rad_field_names[comp_id]) continue;
                int has_a = rad_is_truthy(rad_ecs_has(a, comp_id)) ? 1 : 0;
                int has_b = rad_is_truthy(rad_ecs_has(b, comp_id)) ? 1 : 0;
                if (has_a != has_b) return rad_make_bool(false);
                if (has_a) {
                    RadValue av = rad_ecs_require(a, comp_id);
                    RadValue bv = rad_ecs_require(b, comp_id);
                    if (!rad_is_truthy(rad_eq(av, bv))) return rad_make_bool(false);
                }
            }
            return rad_make_bool(true);
        }
        return rad_make_bool(false);
    }
    if ((a.tag == RV_OPTION_SOME || a.tag == RV_RESULT_OK) &&
        (b.tag == RV_OPTION_SOME || b.tag == RV_RESULT_OK) &&
        a.tag == b.tag) {
        return rad_eq(*a.as.inner, *b.as.inner);
    }
    if (a.tag == RV_OPTION_NONE && b.tag == RV_OPTION_NONE) return rad_make_bool(true);
    if ((a.tag == RV_OPTION_SOME || a.tag == RV_OPTION_NONE || a.tag == RV_RESULT_OK || a.tag == RV_RESULT_ERR) &&
        (b.tag == RV_OPTION_SOME || b.tag == RV_OPTION_NONE || b.tag == RV_RESULT_OK || b.tag == RV_RESULT_ERR) &&
        a.tag != b.tag) {
        return rad_make_bool(false);
    }
    if (a.tag == RV_STRUCT && b.tag == RV_STRUCT) {
        if (!a.as.rst || !b.as.rst) return rad_make_bool(a.as.rst == b.as.rst);
        if (a.as.rst->layout_comp != b.as.rst->layout_comp) return rad_make_bool(false);
        if (a.as.rst->store->len != b.as.rst->store->len) return rad_make_bool(false);
        for (int64_t i = 0; i < a.as.rst->store->len; i++) {
            if (!rad_is_truthy(rad_eq(a.as.rst->store->fields[i], b.as.rst->store->fields[i])))
                return rad_make_bool(false);
        }
        return rad_make_bool(true);
    }
    if (a.tag == RV_BITSET && b.tag == RV_BITSET) return rad_make_bool(a.as.bitset == b.as.bitset);
    return rad_make_bool(rad_to_int(a) == rad_to_int(b));
}
RAD_API RadValue rad_neq(RadValue a, RadValue b) { return rad_make_bool(!rad_is_truthy(rad_eq(a, b))); }
static inline double rad_cmp_num(RadValue v) {
    if (v.tag == RV_FLOAT) return v.as.f;
    return (double)rad_to_int(v);
}
RAD_API RadValue rad_lt(RadValue a, RadValue b) {
    if (a.tag == RV_STR && b.tag == RV_STR)
        return rad_make_bool(strcmp(a.as.str.data ? a.as.str.data : "", b.as.str.data ? b.as.str.data : "") < 0);
    if (a.tag == RV_FLOAT || b.tag == RV_FLOAT)
        return rad_make_bool(rad_cmp_num(a) < rad_cmp_num(b));
    return rad_make_bool(rad_to_int(a) < rad_to_int(b));
}
RAD_API RadValue rad_lte(RadValue a, RadValue b) {
    if (a.tag == RV_STR && b.tag == RV_STR)
        return rad_make_bool(strcmp(a.as.str.data ? a.as.str.data : "", b.as.str.data ? b.as.str.data : "") <= 0);
    if (a.tag == RV_FLOAT || b.tag == RV_FLOAT)
        return rad_make_bool(rad_cmp_num(a) <= rad_cmp_num(b));
    return rad_make_bool(rad_to_int(a) <= rad_to_int(b));
}
RAD_API RadValue rad_gt(RadValue a, RadValue b) {
    if (a.tag == RV_STR && b.tag == RV_STR)
        return rad_make_bool(strcmp(a.as.str.data ? a.as.str.data : "", b.as.str.data ? b.as.str.data : "") > 0);
    if (a.tag == RV_FLOAT || b.tag == RV_FLOAT)
        return rad_make_bool(rad_cmp_num(a) > rad_cmp_num(b));
    return rad_make_bool(rad_to_int(a) > rad_to_int(b));
}
RAD_API RadValue rad_gte(RadValue a, RadValue b) {
    if (a.tag == RV_STR && b.tag == RV_STR)
        return rad_make_bool(strcmp(a.as.str.data ? a.as.str.data : "", b.as.str.data ? b.as.str.data : "") >= 0);
    if (a.tag == RV_FLOAT || b.tag == RV_FLOAT)
        return rad_make_bool(rad_cmp_num(a) >= rad_cmp_num(b));
    return rad_make_bool(rad_to_int(a) >= rad_to_int(b));
}
RAD_API RadValue rad_neg(RadValue a) {
    if (a.tag == RV_FLOAT) return rad_make_float(-a.as.f);
    return rad_make_int(-rad_to_int(a));
}

RAD_API RadValue rad_min(RadValue a, RadValue b) {
    if (a.tag == RV_FLOAT || b.tag == RV_FLOAT) {
        double av = rad_cmp_num(a), bv = rad_cmp_num(b);
        return rad_make_float(av < bv ? av : bv);
    }
    if (a.tag == RV_STR && b.tag == RV_STR) {
        return strcmp(a.as.str.data ? a.as.str.data : "", b.as.str.data ? b.as.str.data : "") <= 0 ? a : b;
    }
    int64_t ai = rad_to_int(a), bi = rad_to_int(b);
    return ai <= bi ? a : b;
}

RAD_API RadValue rad_max(RadValue a, RadValue b) {
    if (a.tag == RV_FLOAT || b.tag == RV_FLOAT) {
        double av = rad_cmp_num(a), bv = rad_cmp_num(b);
        return rad_make_float(av > bv ? av : bv);
    }
    if (a.tag == RV_STR && b.tag == RV_STR) {
        return strcmp(a.as.str.data ? a.as.str.data : "", b.as.str.data ? b.as.str.data : "") >= 0 ? a : b;
    }
    int64_t ai = rad_to_int(a), bi = rad_to_int(b);
    return ai >= bi ? a : b;
}

/* ========== Print / Str / Len ========== */

static void rad_fprint_escaped_cstr(const char *s) {
    printf("\"");
    if (s) {
        for (const char *p = s; *p; p++) {
            if (*p == '"' || *p == '\\')
                putchar('\\');
            putchar(*p);
        }
    }
    printf("\"");
}

static void rad_format_float_text(double x, char *out, size_t out_sz) {
    if (!out || out_sz == 0) return;
    out[0] = '\0';
    for (int p = 2; p <= 17; p++) {
        char tmp[64];
        snprintf(tmp, sizeof(tmp), "%.*g", p, x);
        if (strchr(tmp, 'e') != NULL || strchr(tmp, 'E') != NULL) {
            continue;
        }
        char *end = NULL;
        double parsed = strtod(tmp, &end);
        if (end && *end == '\0' && parsed == x) {
            strncpy(out, tmp, out_sz - 1);
            out[out_sz - 1] = '\0';
            if (strchr(out, '.') == NULL && strchr(out, 'e') == NULL && strchr(out, 'E') == NULL) {
                size_t n = strlen(out);
                if (n + 2 < out_sz) {
                    out[n] = '.';
                    out[n + 1] = '0';
                    out[n + 2] = '\0';
                }
            }
            return;
        }
    }
    snprintf(out, out_sz, "%.17g", x);
    if (strchr(out, '.') == NULL && strchr(out, 'e') == NULL && strchr(out, 'E') == NULL) {
        size_t n = strlen(out);
        if (n + 2 < out_sz) {
            out[n] = '.';
            out[n + 1] = '0';
            out[n + 2] = '\0';
        }
    }
}

static int rad_entity_has_state_variant(RadValue v) {
    if (v.tag != RV_ENTITY) return 0;
    for (int64_t comp_id = 0; comp_id < rad_next_component_id && comp_id < RAD_MAX_COMPONENTS; comp_id++) {
        if (!rad_state_variant_ids[comp_id]) continue;
        if (rad_is_truthy(rad_ecs_has(v, comp_id))) return 1;
    }
    return 0;
}

/* VM print_display: top-level strings are bare; nested strings use quotes (Display). */
static void rad_print_display_inner(RadValue v, int outer_str_unquoted) {
    switch (v.tag) {
    case RV_NIL:
        printf("nil");
        break;
    case RV_INT:
        printf("%lld", (long long)v.as.i);
        break;
    case RV_FLOAT: {
        char fbuf[64];
        rad_format_float_text(v.as.f, fbuf, sizeof(fbuf));
        printf("%s", fbuf);
        break;
    }
    case RV_BOOL:
        printf("%s", v.as.b ? "true" : "false");
        break;
    case RV_STR:
        if (outer_str_unquoted) {
            if (v.as.str.data)
                printf("%s", v.as.str.data);
        } else {
            rad_fprint_escaped_cstr(v.as.str.data ? v.as.str.data : "");
        }
        break;
    case RV_LIST_INT:
        if (!v.as.list_i) {
            printf("[]");
            break;
        }
        printf("[");
        for (int64_t i = 0; i < v.as.list_i->len; i++) {
            if (i > 0) printf(", ");
            printf("%lld", (long long)v.as.list_i->data[i]);
        }
        printf("]");
        break;
    case RV_LIST:
        if (!v.as.list) {
            printf("[]");
            break;
        }
        printf("[");
        for (int64_t i = 0; i < v.as.list->len; i++) {
            if (i > 0) printf(", ");
            rad_print_display_inner(v.as.list->data[i], 0);
        }
        printf("]");
        break;
    case RV_OPTION_SOME:
        printf("Option::Some {value: ");
        rad_print_display_inner(*v.as.inner, 0);
        printf(" }");
        break;
    case RV_OPTION_NONE:
        printf("Option::None {}");
        break;
    case RV_RESULT_OK:
        printf("Result::Ok {value: ");
        rad_print_display_inner(*v.as.inner, 0);
        printf(" }");
        break;
    case RV_RESULT_ERR:
        printf("Result::Err {message: ");
        rad_print_display_inner(*v.as.inner, 0);
        printf(" }");
        break;
    case RV_ENTITY:
    {
        RadValue variant = rad_variant_of(v);
        if (variant.tag == RV_STR && variant.as.str.data && variant.as.str.len > 0) {
            if (rad_entity_has_state_variant(v)) {
                printf("%s", variant.as.str.data);
            } else {
                printf("%s {", variant.as.str.data);
                int first = 1;
                for (int64_t comp_id = 0; comp_id < rad_next_component_id && comp_id < RAD_MAX_COMPONENTS; comp_id++) {
                    const char *fname = rad_field_names[comp_id];
                    if (!fname) continue;
                    if (!rad_is_truthy(rad_ecs_has(v, comp_id))) continue;
                    if (!first) printf(", ");
                    printf("%s: ", fname);
                    rad_print_display_inner(rad_ecs_require(v, comp_id), 0);
                    first = 0;
                }
                if (!first) printf(" ");
                printf("}");
            }
        } else {
            printf("%lld", (long long)v.as.entity_id);
        }
    }
        break;
    case RV_FN:
        printf("<fn:%p>", (void *)v.as.fn.fn_ptr);
        break;
    case RV_BITSET:
        printf("<bitset>");
        break;
    case RV_WORLD_FORK:
        printf("<world_fork>");
        break;
    case RV_BUFFER:
        printf("<buffer>");
        break;
    case RV_TUPLE:
        if (!v.as.tuple) {
            printf("()");
            break;
        }
        printf("(");
        for (int64_t i = 0; i < v.as.tuple->len; i++) {
            if (i > 0) printf(", ");
            rad_print_display_inner(v.as.tuple->data[i], 0);
        }
        if (v.as.tuple->len == 1)
            printf(",");
        printf(")");
        break;
    case RV_MAP: {
        if (!v.as.map) {
            printf("{}");
            break;
        }
        printf("{");
        for (int64_t i = 0; i < v.as.map->len; i++) {
            if (i > 0) printf(", ");
            rad_print_display_inner(v.as.map->keys[i], 0);
            printf(": ");
            rad_print_display_inner(v.as.map->vals[i], 0);
        }
        printf("}");
        break;
    }
    case RV_STRUCT:
        printf("(");
        if (v.as.rst) {
            for (int64_t i = 0; i < v.as.rst->store->len; i++) {
                if (i > 0) printf(", ");
                rad_print_display_inner(v.as.rst->store->fields[i], 0);
            }
        }
        printf(")");
        break;
    default:
        printf("<unknown>");
        break;
    }
}

RAD_API void rad_print(RadValue v) {
    rad_print_display_inner(v, 1);
}

RAD_API RadValue print(RadValue v) {
    return rad_print_many(&v, 1);
}

RAD_API RadValue rad_print_many(RadValue *vals, int64_t n) {
    if (n < 0) n = 0;
    for (int64_t i = 0; i < n; i++) {
        if (i > 0) printf(" ");
        rad_print_display_inner(vals[i], 1);
    }
    printf("\n");
    return rad_make_nil();
}

RAD_API RadValue rad_len(RadValue v) {
    if (v.tag == RV_STR) return rad_make_int(v.as.str.len);
    if (v.tag == RV_LIST_INT && v.as.list_i) return rad_make_int(v.as.list_i->len);
    if (v.tag == RV_LIST && v.as.list) return rad_make_int(v.as.list->len);
    if (v.tag == RV_TUPLE && v.as.tuple) return rad_make_int(v.as.tuple->len);
    if (v.tag == RV_MAP && v.as.map) return rad_make_int(v.as.map->len);
    if (v.tag == RV_STRUCT && v.as.rst && v.as.rst->store) return rad_make_int(v.as.rst->store->len);
    return rad_make_int(0);
}

static void rad_sb_append_bytes(RadBuffer *b, const char *s, int64_t n) {
    if (!b || !s || n <= 0) return;
    int64_t need = b->len + n + 1;
    if (need > b->cap) {
        int64_t new_cap = b->cap == 0 ? 64 : b->cap;
        while (new_cap < need) new_cap *= 2;
        b->data = (char *)rad_arena_realloc(b->data, (size_t)b->cap, (size_t)new_cap);
        b->cap = new_cap;
    }
    memcpy(b->data + b->len, s, (size_t)n);
    b->len += n;
    b->data[b->len] = '\0';
}

static void rad_sb_append_cstr(RadBuffer *b, const char *s) {
    if (!s) return;
    rad_sb_append_bytes(b, s, (int64_t)strlen(s));
}

static void rad_sb_append_char(RadBuffer *b, char c) {
    rad_sb_append_bytes(b, &c, 1);
}

static void rad_sb_append_escaped_cstr(RadBuffer *b, const char *s) {
    rad_sb_append_char(b, '"');
    if (s) {
        for (const char *p = s; *p; p++) {
            if (*p == '"' || *p == '\\') {
                rad_sb_append_char(b, '\\');
            }
            rad_sb_append_char(b, *p);
        }
    }
    rad_sb_append_char(b, '"');
}

static void rad_stringify_inner(RadValue v, int outer_str_unquoted, RadBuffer *b) {
    char buf[128];
    switch (v.tag) {
    case RV_NIL:
        rad_sb_append_cstr(b, "nil");
        break;
    case RV_BOOL:
        rad_sb_append_cstr(b, v.as.b ? "true" : "false");
        break;
    case RV_INT:
        snprintf(buf, sizeof(buf), "%lld", (long long)v.as.i);
        rad_sb_append_cstr(b, buf);
        break;
    case RV_FLOAT:
        rad_format_float_text(v.as.f, buf, sizeof(buf));
        rad_sb_append_cstr(b, buf);
        break;
    case RV_STR:
        if (outer_str_unquoted) {
            rad_sb_append_cstr(b, v.as.str.data ? v.as.str.data : "");
        } else {
            rad_sb_append_escaped_cstr(b, v.as.str.data ? v.as.str.data : "");
        }
        break;
    case RV_LIST_INT:
        rad_sb_append_char(b, '[');
        if (v.as.list_i) {
            for (int64_t i = 0; i < v.as.list_i->len; i++) {
                if (i > 0) rad_sb_append_cstr(b, ", ");
                snprintf(buf, sizeof(buf), "%lld", (long long)v.as.list_i->data[i]);
                rad_sb_append_cstr(b, buf);
            }
        }
        rad_sb_append_char(b, ']');
        break;
    case RV_LIST:
        rad_sb_append_char(b, '[');
        if (v.as.list) {
            for (int64_t i = 0; i < v.as.list->len; i++) {
                if (i > 0) rad_sb_append_cstr(b, ", ");
                rad_stringify_inner(v.as.list->data[i], 0, b);
            }
        }
        rad_sb_append_char(b, ']');
        break;
    case RV_OPTION_SOME:
        rad_sb_append_cstr(b, "Option::Some {value: ");
        rad_stringify_inner(*v.as.inner, 0, b);
        rad_sb_append_cstr(b, " }");
        break;
    case RV_OPTION_NONE:
        rad_sb_append_cstr(b, "Option::None {}");
        break;
    case RV_RESULT_OK:
        rad_sb_append_cstr(b, "Result::Ok {value: ");
        rad_stringify_inner(*v.as.inner, 0, b);
        rad_sb_append_cstr(b, " }");
        break;
    case RV_RESULT_ERR:
        rad_sb_append_cstr(b, "Result::Err {message: ");
        rad_stringify_inner(*v.as.inner, 0, b);
        rad_sb_append_cstr(b, " }");
        break;
    case RV_ENTITY: {
        RadValue variant = rad_variant_of(v);
        if (variant.tag == RV_STR && variant.as.str.data && variant.as.str.len > 0) {
            if (rad_entity_has_state_variant(v)) {
                rad_sb_append_cstr(b, variant.as.str.data);
            } else {
                rad_sb_append_cstr(b, variant.as.str.data);
                rad_sb_append_cstr(b, " {");
                int first = 1;
                for (int64_t comp_id = 0; comp_id < rad_next_component_id && comp_id < RAD_MAX_COMPONENTS; comp_id++) {
                    const char *fname = rad_field_names[comp_id];
                    if (!fname) continue;
                    if (!rad_is_truthy(rad_ecs_has(v, comp_id))) continue;
                    if (!first) rad_sb_append_cstr(b, ", ");
                    rad_sb_append_cstr(b, fname);
                    rad_sb_append_cstr(b, ": ");
                    rad_stringify_inner(rad_ecs_require(v, comp_id), 0, b);
                    first = 0;
                }
                if (!first) rad_sb_append_char(b, ' ');
                rad_sb_append_char(b, '}');
            }
        } else {
            snprintf(buf, sizeof(buf), "%lld", (long long)v.as.entity_id);
            rad_sb_append_cstr(b, buf);
        }
        break;
    }
    case RV_TUPLE:
        rad_sb_append_char(b, '(');
        if (v.as.tuple) {
            for (int64_t i = 0; i < v.as.tuple->len; i++) {
                if (i > 0) rad_sb_append_cstr(b, ", ");
                rad_stringify_inner(v.as.tuple->data[i], 0, b);
            }
            if (v.as.tuple->len == 1) {
                rad_sb_append_char(b, ',');
            }
        }
        rad_sb_append_char(b, ')');
        break;
    case RV_MAP:
        rad_sb_append_char(b, '{');
        if (v.as.map) {
            for (int64_t i = 0; i < v.as.map->len; i++) {
                if (i > 0) rad_sb_append_cstr(b, ", ");
                rad_stringify_inner(v.as.map->keys[i], 0, b);
                rad_sb_append_cstr(b, ": ");
                rad_stringify_inner(v.as.map->vals[i], 0, b);
            }
        }
        rad_sb_append_char(b, '}');
        break;
    case RV_BITSET:
        rad_sb_append_cstr(b, "<bitset>");
        break;
    case RV_WORLD_FORK:
        rad_sb_append_cstr(b, "<world_fork>");
        break;
    case RV_BUFFER:
        rad_sb_append_cstr(b, "<buffer>");
        break;
    case RV_FN:
        rad_sb_append_cstr(b, "<fn>");
        break;
    case RV_STRUCT:
        rad_sb_append_char(b, '(');
        if (v.as.rst && v.as.rst->store) {
            for (int64_t i = 0; i < v.as.rst->store->len; i++) {
                if (i > 0) rad_sb_append_cstr(b, ", ");
                rad_stringify_inner(v.as.rst->store->fields[i], 0, b);
            }
        }
        rad_sb_append_char(b, ')');
        break;
    default:
        rad_sb_append_cstr(b, "<unknown>");
        break;
    }
}

RAD_API RadValue str(RadValue v) {
    RadBuffer b = {0};
    rad_stringify_inner(v, 1, &b);
    if (!b.data) {
        return rad_make_str("");
    }
    return rad_make_str(b.data);
}

RAD_API RadValue rad_assert(RadValue cond, RadValue msg) {
    if (!rad_is_truthy(cond)) {
        fprintf(stderr, "assertion failed: ");
        rad_print(msg);
        fprintf(stderr, "\n");
        exit(1);
    }
    return rad_make_nil();
}

RAD_API RadValue range(RadValue start, RadValue stop) {
    int64_t a = rad_to_int(start), b = rad_to_int(stop);
    RadIntList *xs = rad_list_int_new();
    for (int64_t i = a; i < b; i++) rad_list_int_push(xs, i);
    RadValue v; v.tag = RV_LIST_INT; v.as.list_i = xs;
    return v;
}

RAD_API RadValue rad_range_step(RadValue start, RadValue stop, RadValue step) {
    int64_t a = rad_to_int(start), b = rad_to_int(stop), s = rad_to_int(step);
    RadIntList *xs = rad_list_int_new();
    if (s == 0) {
        RadValue empty; empty.tag = RV_LIST_INT; empty.as.list_i = xs;
        return empty;
    }
    if (s > 0) {
        for (int64_t i = a; i < b; i += s) rad_list_int_push(xs, i);
    } else {
        for (int64_t i = a; i > b; i += s) rad_list_int_push(xs, i);
    }
    RadValue v; v.tag = RV_LIST_INT; v.as.list_i = xs;
    return v;
}

/* ========== RadList (arena-backed) ========== */

RAD_API RadList *rad_list_new(void) {
    RadList *xs = (RadList *)rad_arena_alloc(sizeof(RadList));
    xs->data = NULL; xs->len = 0; xs->cap = 0;
    return xs;
}

RAD_API void rad_list_reserve(RadList *xs, int64_t need) {
    if (!xs || xs->cap >= need) return;
    int64_t cap = xs->cap == 0 ? 8 : xs->cap;
    while (cap < need) cap *= 2;
    xs->data = (RadValue *)rad_arena_realloc(xs->data,
        (size_t)xs->cap * sizeof(RadValue), (size_t)cap * sizeof(RadValue));
    xs->cap = cap;
}

RAD_API void rad_list_push(RadList *xs, RadValue v) {
    if (!xs) return;
    rad_list_reserve(xs, xs->len + 1);
    xs->data[xs->len++] = v;
}

RAD_API RadValue rad_list_get(RadList *xs, int64_t i) {
    if (!xs || i < 0 || i >= xs->len) return rad_make_nil();
    return xs->data[i];
}

RAD_API void rad_list_set(RadList *xs, int64_t i, RadValue v) {
    if (!xs || i < 0 || i >= xs->len) return;
    xs->data[i] = v;
}

RAD_API RadValue rad_make_list(void) {
    RadValue v; v.tag = RV_LIST; v.as.list = rad_list_new(); return v;
}

RAD_API RadValue rad_list_literal(RadValue *elements, int64_t len) {
    RadValue v = rad_make_list();
    for (int64_t i = 0; i < len; i++) rad_list_push(v.as.list, elements[i]);
    return v;
}

RAD_API RadValue rad_push(RadValue lst, RadValue val) {
    if (lst.tag == RV_LIST && lst.as.list) {
        RadList *xs = rad_list_new();
        for (int64_t i = 0; i < lst.as.list->len; i++) {
            rad_list_push(xs, lst.as.list->data[i]);
        }
        rad_list_push(xs, val);
        RadValue out; out.tag = RV_LIST; out.as.list = xs;
        return out;
    } else if (lst.tag == RV_LIST_INT && lst.as.list_i) {
        RadIntList *xs = rad_list_int_new();
        for (int64_t i = 0; i < lst.as.list_i->len; i++) {
            rad_list_int_push(xs, lst.as.list_i->data[i]);
        }
        rad_list_int_push(xs, rad_to_int(val));
        RadValue out; out.tag = RV_LIST_INT; out.as.list_i = xs;
        return out;
    } else {
        fprintf(stderr, "rad runtime: push() expects a list\n");
        exit(1);
    }
}

/* ========== O(1) ECS with Bitmask Signatures ========== */

typedef struct {
    RadRefColumn *column;
} RadComponentStore;

RAD_API RadComponentStore rad_components[RAD_MAX_COMPONENTS];
RAD_API int64_t rad_next_component_id = 0;
static const char *rad_field_names[RAD_MAX_COMPONENTS] = {0};
static unsigned char rad_state_variant_ids[RAD_MAX_COMPONENTS] = {0};

RAD_API uint64_t *rad_entity_masks = NULL;
RAD_API uint64_t *rad_entity_alive = NULL;
RAD_API int64_t rad_mask_cap = 0;
RAD_API int64_t rad_mask_words = 1;
RAD_API int64_t rad_next_entity = 0;

RAD_API int64_t *rad_free_ids = NULL;
RAD_API int64_t rad_free_count = 0;
RAD_API int64_t rad_free_cap = 0;

static RadRefU64Array *rad_masks_ref = NULL;
static RadRefU64Array *rad_alive_ref = NULL;

static RadRefU64Array *rad_ref_u64_new(int64_t len_words) {
    if (len_words <= 0) return NULL;
    RadRefU64Array *ref = (RadRefU64Array *)rad_arena_alloc(sizeof(RadRefU64Array));
    ref->data = (uint64_t *)rad_arena_alloc((size_t)len_words * sizeof(uint64_t));
    ref->len_words = len_words;
    ref->refcount = 1;
    return ref;
}

static RadRefU64Array *rad_ref_u64_clone(const RadRefU64Array *src, int64_t len_words) {
    RadRefU64Array *dst = rad_ref_u64_new(len_words);
    if (!dst || !src || !src->data) return dst;
    int64_t n = src->len_words < len_words ? src->len_words : len_words;
    if (n > 0) {
        memcpy(dst->data, src->data, (size_t)n * sizeof(uint64_t));
    }
    return dst;
}

static void rad_ref_u64_retain(RadRefU64Array *ref) {
    if (ref) ref->refcount++;
}

static void rad_ref_u64_release(RadRefU64Array *ref) {
    if (ref && ref->refcount > 0) ref->refcount--;
}

static void rad_attach_masks_ref(RadRefU64Array *ref) {
    rad_masks_ref = ref;
    rad_entity_masks = ref ? ref->data : NULL;
}

static void rad_attach_alive_ref(RadRefU64Array *ref) {
    rad_alive_ref = ref;
    rad_entity_alive = ref ? ref->data : NULL;
}

static void rad_masks_ensure_unique_for_write(void) {
    if (rad_masks_ref && rad_masks_ref->refcount > 1) {
        RadRefU64Array *new_ref = rad_ref_u64_clone(rad_masks_ref, rad_masks_ref->len_words);
        rad_ref_u64_release(rad_masks_ref);
        rad_attach_masks_ref(new_ref);
    }
    if (rad_alive_ref && rad_alive_ref->refcount > 1) {
        RadRefU64Array *new_ref = rad_ref_u64_clone(rad_alive_ref, rad_alive_ref->len_words);
        rad_ref_u64_release(rad_alive_ref);
        rad_attach_alive_ref(new_ref);
    }
}

static RadRefColumn *rad_refcol_new_with_capacity(int64_t capacity) {
    if (capacity < 0) capacity = 0;
    RadRefColumn *col = (RadRefColumn *)rad_arena_alloc(sizeof(RadRefColumn));
    col->capacity = capacity;
    col->refcount = 1;
    col->data = capacity > 0
        ? (RadValue *)rad_arena_alloc((size_t)capacity * sizeof(RadValue))
        : NULL;
    return col;
}

static RadRefColumn *rad_refcol_clone_for_cow(const RadRefColumn *src, int64_t min_capacity) {
    int64_t cap = src ? src->capacity : 0;
    if (cap < min_capacity) {
        cap = cap == 0 ? 256 : cap;
        while (cap < min_capacity) cap *= 2;
    }
    RadRefColumn *dst = rad_refcol_new_with_capacity(cap);
    if (src && src->data && src->capacity > 0) {
        for (int64_t i = 0; i < src->capacity; i++) {
            dst->data[i] = rad_value_deep_copy(src->data[i]);
        }
    }
    return dst;
}

static void rad_refcol_retain(RadRefColumn *col) {
    if (col) col->refcount++;
}

static void rad_refcol_release(RadRefColumn *col) {
    if (col && col->refcount > 0) col->refcount--;
}

static int64_t rad_alloc_entity_id(void) {
    int64_t id;
    if (rad_free_count > 0) {
        id = rad_free_ids[--rad_free_count];
    } else {
        if (rad_next_entity == 9223372036854775807LL) {
            fprintf(stderr, "rad runtime: entity ID overflow\n");
            exit(1);
        }
        id = rad_next_entity++;
    }
    return id;
}

static void rad_push_free_entity_id(int64_t eid) {
    if (rad_free_count >= rad_free_cap) {
        int64_t new_cap = rad_free_cap == 0 ? 256 : rad_free_cap * 2;
        rad_free_ids = (int64_t *)rad_arena_realloc(rad_free_ids,
            (size_t)rad_free_cap * sizeof(int64_t), (size_t)new_cap * sizeof(int64_t));
        rad_free_cap = new_cap;
    }
    rad_free_ids[rad_free_count++] = eid;
}

static void rad_grow_mask_width(int64_t new_words) {
    if (new_words <= rad_mask_words) return;
    int64_t old_words = rad_mask_words;
    if (rad_mask_cap > 0) {
        int64_t total_words = rad_mask_cap * new_words;
        RadRefU64Array *new_ref = rad_ref_u64_new(total_words);
        if (new_ref && rad_entity_masks) {
            for (int64_t e = 0; e < rad_mask_cap; e++) {
                memcpy(new_ref->data + e * new_words,
                       rad_entity_masks + e * old_words,
                       (size_t)old_words * sizeof(uint64_t));
            }
        }
        RadRefU64Array *old_ref = rad_masks_ref;
        rad_attach_masks_ref(new_ref);
        rad_ref_u64_release(old_ref);
    }
    rad_mask_words = new_words;
}

RAD_API int64_t rad_register_component(void) {
    int64_t id = rad_next_component_id++;
    if (id >= RAD_MAX_COMPONENTS) {
        fprintf(stderr, "rad runtime: too many component types (max %d)\n", RAD_MAX_COMPONENTS);
        exit(1);
    }
    int64_t needed_words = (id >> 6) + 1;
    if (needed_words > rad_mask_words) rad_grow_mask_width(needed_words);
    rad_components[id].column = NULL;
    return id;
}

RAD_API void rad_register_field_name(int64_t field_id, const char *name) {
    if (field_id < 0 || field_id >= RAD_MAX_COMPONENTS) return;
    rad_field_names[field_id] = name;
}

#define RAD_MAX_FIELD_LAYOUT_RULES 8192
static int64_t rad_layout_rule_comp[RAD_MAX_FIELD_LAYOUT_RULES];
static int64_t rad_layout_rule_field[RAD_MAX_FIELD_LAYOUT_RULES];
static int64_t rad_layout_rule_slot[RAD_MAX_FIELD_LAYOUT_RULES];
static int64_t rad_layout_rule_count = 0;

RAD_API void rad_register_field_layout(int64_t field_id, int64_t parent_comp_id, int64_t ordinal) {
    if (field_id < 0 || field_id >= RAD_MAX_COMPONENTS) return;
    if (rad_layout_rule_count >= RAD_MAX_FIELD_LAYOUT_RULES) {
        fprintf(stderr, "rad runtime: too many field layout rules (max %d)\n", RAD_MAX_FIELD_LAYOUT_RULES);
        exit(1);
    }
    int64_t i = rad_layout_rule_count++;
    rad_layout_rule_comp[i] = parent_comp_id;
    rad_layout_rule_field[i] = field_id;
    rad_layout_rule_slot[i] = ordinal;
}

static int64_t rad_struct_resolve_slot(int64_t layout_comp, int64_t field_id) {
    if (layout_comp < 0 || field_id < 0) return -1;
    for (int64_t i = rad_layout_rule_count - 1; i >= 0; i--) {
        if (rad_layout_rule_comp[i] == layout_comp && rad_layout_rule_field[i] == field_id)
            return rad_layout_rule_slot[i];
    }
    return -1;
}

RAD_API void rad_register_state_variant(int64_t comp_id) {
    if (comp_id < 0 || comp_id >= RAD_MAX_COMPONENTS) return;
    rad_state_variant_ids[comp_id] = 1;
}

RAD_API void rad_ensure_masks(int64_t eid) {
    if (eid < rad_mask_cap) return;
    int64_t new_cap = rad_mask_cap == 0 ? 4096 : rad_mask_cap;
    while (new_cap <= eid) new_cap *= 2;
    int64_t old_cap = rad_mask_cap;
    int64_t new_mask_words = new_cap * rad_mask_words;
    int64_t new_alive_words = (new_cap + 63) / 64;
    int64_t old_alive_words = (old_cap + 63) / 64;

    RadRefU64Array *new_masks = rad_ref_u64_new(new_mask_words);
    if (new_masks && rad_entity_masks && old_cap > 0) {
        size_t copy_words = (size_t)old_cap * (size_t)rad_mask_words;
        memcpy(new_masks->data, rad_entity_masks, copy_words * sizeof(uint64_t));
    }

    RadRefU64Array *new_alive = rad_ref_u64_new(new_alive_words);
    if (new_alive && rad_entity_alive && old_alive_words > 0) {
        memcpy(new_alive->data, rad_entity_alive, (size_t)old_alive_words * sizeof(uint64_t));
    }

    RadRefU64Array *old_masks = rad_masks_ref;
    RadRefU64Array *old_alive = rad_alive_ref;
    rad_attach_masks_ref(new_masks);
    rad_attach_alive_ref(new_alive);
    rad_ref_u64_release(old_masks);
    rad_ref_u64_release(old_alive);

    rad_mask_cap = new_cap;
}

RAD_API inline bool rad_is_alive(int64_t eid) {
    if (eid < 0 || eid >= rad_mask_cap) return false;
    return (rad_entity_alive[eid / 64] & (1ULL << (eid % 64))) != 0;
}

RAD_API inline void rad_set_alive(int64_t eid) {
    if (eid < 0 || eid >= rad_mask_cap) return;
    rad_masks_ensure_unique_for_write();
    rad_entity_alive[eid / 64] |= (1ULL << (eid % 64));
}

RAD_API inline void rad_clear_alive(int64_t eid) {
    if (eid < 0 || eid >= rad_mask_cap) return;
    rad_masks_ensure_unique_for_write();
    rad_entity_alive[eid / 64] &= ~(1ULL << (eid % 64));
}

RAD_API void rad_component_ensure(RadComponentStore *store, int64_t eid) {
    int64_t min_cap = eid + 1;
    if (min_cap < 0) min_cap = 0;
    if (!store->column) {
        int64_t cap = min_cap == 0 ? 0 : 256;
        while (cap < min_cap) cap *= 2;
        store->column = rad_refcol_new_with_capacity(cap);
        return;
    }
    if (store->column->refcount == 1 && eid < store->column->capacity) return;
    RadRefColumn *old = store->column;
    store->column = rad_refcol_clone_for_cow(old, min_cap);
    rad_refcol_release(old);
}

RAD_API RadValue rad_make_entity(int64_t id) {
    RadValue v; v.tag = RV_ENTITY; v.as.entity_id = id; return v;
}

RAD_API RadValue rad_spawn(void) {
    int64_t id = rad_alloc_entity_id();
    rad_ensure_masks(id);
    rad_set_alive(id);
    return rad_make_entity(id);
}

RAD_API RadValue rad_entity_names;
RAD_API RadValue rad_entity_id_to_name;
RAD_API bool rad_entity_names_init = false;

typedef struct {
    const char *event_name;
    RadValue payload;
} RadPendingEvent;

typedef struct {
    const char *event_name;
    RadEventHandlerFn fn;
    int once;
    int active;
} RadEventHandlerSlot;

static RadPendingEvent *rad_event_queue = NULL;
static int64_t rad_event_queue_len = 0;
static int64_t rad_event_queue_cap = 0;

static RadEventHandlerSlot *rad_event_handlers = NULL;
static int64_t rad_event_handlers_len = 0;
static int64_t rad_event_handlers_cap = 0;

typedef struct {
    int64_t from_comp;
    const char *event_name;
    int64_t to_comp;
    int guard_true;
} RadTransitionRule;

static RadTransitionRule *rad_transition_rules = NULL;
static int64_t rad_transition_rules_len = 0;
static int64_t rad_transition_rules_cap = 0;
static int64_t rad_gc_collect_calls = 0;

static int rad_task_context_depth = 0;
static int64_t rad_simulation_depth = 0;

static void rad_ensure_entity_name_maps(void) {
    if (!rad_entity_names_init) {
        rad_entity_names = rad_make_map();
        rad_entity_id_to_name = rad_make_map();
        rad_entity_names_init = true;
    }
}

RAD_API RadValue rad_spawn_named(RadValue name) {
    /* Empty name: unnamed entity, do not pollute name maps (matches VM spawn_entity). */
    if (name.tag == RV_STR && name.as.str.len == 0) {
        return rad_spawn();
    }
    rad_ensure_entity_name_maps();
    RadValue old_ent = rad_map_get(rad_entity_names, name);
    if (old_ent.tag == RV_ENTITY) {
        int64_t old_eid = old_ent.as.entity_id;
        rad_map_remove(rad_entity_id_to_name, rad_make_int(old_eid));
    }
    int64_t id = rad_alloc_entity_id();
    rad_ensure_masks(id);
    rad_set_alive(id);
    RadValue ent = rad_make_entity(id);
    rad_map_set(rad_entity_names, name, ent);
    rad_map_set(rad_entity_id_to_name, rad_make_int(id), name);
    return ent;
}

RAD_API RadValue rad_get_entity(RadValue name) {
    rad_ensure_entity_name_maps();
    return rad_map_get(rad_entity_names, name);
}

static RadValue rad_make_some(RadValue inner) {
    RadValue v;
    v.tag = RV_OPTION_SOME;
    v.as.inner = (RadValue *)rad_arena_alloc(sizeof(RadValue));
    *v.as.inner = inner;
    return v;
}

static RadValue rad_make_none(void) {
    RadValue v;
    v.tag = RV_OPTION_NONE;
    v.as.inner = NULL;
    return v;
}

static RadValue rad_make_result_ok(RadValue inner) {
    RadValue v;
    v.tag = RV_RESULT_OK;
    v.as.inner = (RadValue *)rad_arena_alloc(sizeof(RadValue));
    *v.as.inner = inner;
    return v;
}

static RadValue rad_make_result_err(RadValue msg) {
    RadValue v;
    v.tag = RV_RESULT_ERR;
    v.as.inner = (RadValue *)rad_arena_alloc(sizeof(RadValue));
    *v.as.inner = msg;
    return v;
}

RAD_API inline void rad_mask_set(int64_t eid, int64_t comp_id) {
    rad_entity_masks[eid * rad_mask_words + (comp_id >> 6)] |= (1ULL << (comp_id & 63));
}

RAD_API inline bool rad_mask_has(int64_t eid, int64_t comp_id) {
    if (eid < 0 || eid >= rad_mask_cap) return false;
    return (rad_entity_masks[eid * rad_mask_words + (comp_id >> 6)] & (1ULL << (comp_id & 63))) != 0;
}

RAD_API RadValue rad_despawn(RadValue ent) {
    if (ent.tag != RV_ENTITY) return rad_make_bool(false);
    int64_t eid = ent.as.entity_id;
    if (eid < 0 || eid >= rad_mask_cap) return rad_make_bool(false);

    if (!rad_is_alive(eid)) return rad_make_bool(false);

    if (rad_entity_names_init) {
        RadValue key = rad_make_int(eid);
        RadValue nm = rad_map_get(rad_entity_id_to_name, key);
        if (nm.tag == RV_STR && nm.as.str.data) {
            rad_map_remove(rad_entity_names, nm);
            rad_map_remove(rad_entity_id_to_name, key);
        }
    }

    for (int64_t c = 0; c < rad_next_component_id; c++) {
        if (rad_mask_has(eid, c)) {
            RadComponentStore *store = &rad_components[c];
            if (store->column && eid < store->column->capacity) {
                rad_component_ensure(store, eid);
                store->column->data[eid] = rad_make_nil();
            }
        }
    }

    rad_masks_ensure_unique_for_write();
    for (int64_t w = 0; w < rad_mask_words; w++) {
        rad_entity_masks[eid * rad_mask_words + w] = 0;
    }
    rad_clear_alive(eid);
    rad_push_free_entity_id(eid);
    return rad_make_bool(true);
}

/* Phase A: skip COW + deep_copy when new value is structurally equal to stored cell. */
static RadValue rad_storage_get_raw(int64_t eid, int64_t comp_id) {
    if (eid < 0 || !rad_is_alive(eid) || !rad_mask_has(eid, comp_id)) return rad_make_nil();
    if (comp_id < 0 || comp_id >= rad_next_component_id) return rad_make_nil();
    RadComponentStore *st = &rad_components[comp_id];
    if (!st->column || eid >= st->column->capacity) return rad_make_nil();
    return st->column->data[eid];
}

static bool rad_storage_value_equal(RadValue a, RadValue b);

static bool rad_entity_ecs_equal(int64_t ea, int64_t eb) {
    if (ea == eb) return true;
    if (!rad_is_alive(ea) || !rad_is_alive(eb)) return false;
    RadValue a_ent = rad_make_entity(ea);
    RadValue b_ent = rad_make_entity(eb);
    for (int64_t comp_id = 0; comp_id < rad_next_component_id && comp_id < RAD_MAX_COMPONENTS; comp_id++) {
        if (!rad_field_names[comp_id]) continue;
        int ha = rad_mask_has(ea, comp_id);
        int hb = rad_mask_has(eb, comp_id);
        if (ha != hb) return false;
        if (ha) {
            RadValue av = rad_storage_get_raw(ea, comp_id);
            RadValue bv = rad_storage_get_raw(eb, comp_id);
            if (!rad_storage_value_equal(av, bv)) return false;
        }
    }
    return true;
}

static bool rad_storage_value_equal(RadValue a, RadValue b) {
    if (a.tag != b.tag) return false;
    switch (a.tag) {
    case RV_NIL:
        return true;
    case RV_INT:
        return a.as.i == b.as.i;
    case RV_FLOAT:
        return a.as.f == b.as.f;
    case RV_BOOL:
        return a.as.b == b.as.b;
    case RV_STR:
        return rad_str_eq(a.as.str, b.as.str);
    case RV_ENTITY:
        return rad_entity_ecs_equal(a.as.entity_id, b.as.entity_id);
    case RV_STRUCT: {
        if (!a.as.rst || !b.as.rst) return a.as.rst == b.as.rst;
        if (a.as.rst->layout_comp != b.as.rst->layout_comp) return false;
        if (a.as.rst->store->len != b.as.rst->store->len) return false;
        for (int64_t i = 0; i < a.as.rst->store->len; i++) {
            if (!rad_storage_value_equal(a.as.rst->store->fields[i], b.as.rst->store->fields[i])) return false;
        }
        return true;
    }
    case RV_BITSET:
        return a.as.bitset == b.as.bitset;
    default:
        return rad_is_truthy(rad_eq(a, b));
    }
}

RAD_API RadValue rad_ecs_set(RadValue ent, int64_t comp_id, RadValue val) {
    if (ent.tag != RV_ENTITY) return rad_make_nil();
    int64_t eid = ent.as.entity_id;
    if (!rad_is_alive(eid)) return rad_make_nil();
    rad_ensure_masks(eid);
    RadComponentStore *store = &rad_components[comp_id];
    if (comp_id >= 0 && comp_id < rad_next_component_id &&
        rad_mask_has(eid, comp_id) &&
        store->column && eid >= 0 && eid < store->column->capacity) {
        RadValue old = store->column->data[eid];
        if (rad_storage_value_equal(old, val)) return rad_make_nil();
    }
    rad_mask_set(eid, comp_id);
    rad_component_ensure(store, eid);
    store->column->data[eid] = rad_value_deep_copy(val);
    return rad_make_nil();
}

RAD_API RadValue rad_ecs_has(RadValue ent, int64_t comp_id) {
    if (ent.tag != RV_ENTITY) return rad_make_bool(false);
    if (!rad_is_alive(ent.as.entity_id)) return rad_make_bool(false);
    return rad_make_bool(rad_mask_has(ent.as.entity_id, comp_id));
}

RAD_API RadValue rad_ecs_get(RadValue ent, int64_t comp_id) {
    if (ent.tag != RV_ENTITY) return rad_make_none();
    int64_t eid = ent.as.entity_id;
    if (!rad_is_alive(eid)) return rad_make_none();
    if (!rad_mask_has(eid, comp_id)) return rad_make_none();
    RadComponentStore *store = &rad_components[comp_id];
    if (!store->column || eid < 0 || eid >= store->column->capacity) return rad_make_none();
    return rad_make_some(rad_value_deep_copy(store->column->data[eid]));
}

RAD_API RadValue rad_ecs_require(RadValue ent, int64_t comp_id) {
    if (ent.tag != RV_ENTITY) {
        fprintf(stderr, "rad runtime: require() expects entity, got tag %d (component %lld)\n",
                ent.tag, (long long)comp_id);
        exit(1);
    }
    int64_t eid = ent.as.entity_id;
    if (!rad_is_alive(eid)) {
        fprintf(stderr, "rad runtime: require() called on dead entity %lld\n", (long long)eid);
        exit(1);
    }
    if (!rad_mask_has(eid, comp_id)) {
        fprintf(stderr, "rad runtime: require() missing component %lld on entity %lld\n",
                (long long)comp_id, (long long)eid);
        fprintf(stderr, "rad runtime: entity %lld has components:", (long long)eid);
        for (int64_t i = 0; i < rad_next_component_id; i++) {
            if (rad_mask_has(eid, i)) {
                fprintf(stderr, " %lld", (long long)i);
            }
        }
        fprintf(stderr, "\n");
        exit(1);
    }
    RadComponentStore *store = &rad_components[comp_id];
    if (!store->column || eid < 0 || eid >= store->column->capacity) {
        fprintf(stderr, "rad runtime: require() internal storage missing for component %lld on entity %lld\n",
                (long long)comp_id, (long long)eid);
        exit(1);
    }
    return rad_value_deep_copy(store->column->data[eid]);
}

RAD_API RadValue rad_ecs_remove(RadValue ent, int64_t comp_id) {
    if (ent.tag != RV_ENTITY) return rad_make_bool(false);
    int64_t eid = ent.as.entity_id;
    if (!rad_is_alive(eid)) return rad_make_bool(false);
    if (comp_id < 0 || comp_id >= rad_next_component_id) return rad_make_bool(false);
    bool had = rad_mask_has(eid, comp_id);
    if (eid < rad_mask_cap) {
        rad_masks_ensure_unique_for_write();
        rad_entity_masks[eid * rad_mask_words + (comp_id >> 6)] &= ~(1ULL << (comp_id & 63));
        RadComponentStore *store = &rad_components[comp_id];
        if (store->column && store->column->capacity > eid) {
            rad_component_ensure(store, eid);
            store->column->data[eid] = rad_make_nil();
        }
    }
    return rad_make_bool(had);
}

RAD_API RadValue rad_ecs_merge(RadValue dest, RadValue src) {
    if (dest.tag != RV_ENTITY || src.tag != RV_ENTITY) return rad_make_nil();
    int64_t did = dest.as.entity_id;
    int64_t sid = src.as.entity_id;
    if (!rad_is_alive(did) || !rad_is_alive(sid)) return rad_make_nil();
    rad_ensure_masks(did);
    rad_masks_ensure_unique_for_write();
    for (int64_t w = 0; w < rad_mask_words; w++) {
        uint64_t bits = rad_entity_masks[sid * rad_mask_words + w];
        while (bits) {
            int bit = 0;
            uint64_t tmp = bits;
            while (!(tmp & 1)) { tmp >>= 1; bit++; }
            int64_t comp_id = w * 64 + bit;
            if (comp_id < 0 || comp_id >= rad_next_component_id) {
                bits &= bits - 1;
                continue;
            }
            RadComponentStore *store = &rad_components[comp_id];
            if (!store->column || sid >= store->column->capacity) {
                bits &= bits - 1;
                continue;
            }
            rad_component_ensure(store, did);
            store->column->data[did] = rad_value_deep_copy(store->column->data[sid]);
            bits &= bits - 1;
        }
        rad_entity_masks[did * rad_mask_words + w] |= rad_entity_masks[sid * rad_mask_words + w];
    }
    return rad_make_nil();
}

RAD_API RadValue rad_query(int64_t *comp_ids, int64_t n_comps) {
    RadValue result = rad_make_list();
    for (int64_t eid = 0; eid < rad_next_entity; eid++) {
        if (eid >= rad_mask_cap) break;
        if (!rad_is_alive(eid)) continue;
        bool match = true;
        for (int64_t ci = 0; ci < n_comps; ci++) {
            if (!rad_mask_has(eid, comp_ids[ci])) { match = false; break; }
        }
        if (match) rad_list_push(result.as.list, rad_make_entity(eid));
    }
    return result;
}

/* ========== String Builtins ========== */

static const char *rad_type_name_for_value(RadValue v) {
    switch (v.tag) {
    case RV_NIL: return "nil";
    case RV_INT: return "int";
    case RV_FLOAT: return "float";
    case RV_BOOL: return "bool";
    case RV_STR: return "str";
    case RV_LIST_INT: case RV_LIST: return "list";
    case RV_TUPLE: return "tuple";
    case RV_MAP: return "map";
    case RV_STRUCT: return "struct";
    case RV_ENTITY: return "entity";
    case RV_FN: return "fn";
    case RV_BITSET: return "bitset";
    case RV_WORLD_FORK: return "world_fork";
    case RV_BUFFER: return "buffer";
    default: return "unknown";
    }
}

static int rad_is_valid_utf8_slice(const unsigned char *s, int64_t len) {
    int64_t i = 0;
    while (i < len) {
        unsigned char c = s[i];
        if (c <= 0x7F) {
            i += 1;
            continue;
        }
        if ((c & 0xE0) == 0xC0) {
            if (i + 1 >= len) return 0;
            unsigned char c1 = s[i + 1];
            if ((c1 & 0xC0) != 0x80) return 0;
            if (c < 0xC2) return 0; /* overlong */
            i += 2;
            continue;
        }
        if ((c & 0xF0) == 0xE0) {
            if (i + 2 >= len) return 0;
            unsigned char c1 = s[i + 1];
            unsigned char c2 = s[i + 2];
            if ((c1 & 0xC0) != 0x80 || (c2 & 0xC0) != 0x80) return 0;
            if (c == 0xE0 && c1 < 0xA0) return 0; /* overlong */
            if (c == 0xED && c1 >= 0xA0) return 0; /* surrogate */
            i += 3;
            continue;
        }
        if ((c & 0xF8) == 0xF0) {
            if (i + 3 >= len) return 0;
            unsigned char c1 = s[i + 1];
            unsigned char c2 = s[i + 2];
            unsigned char c3 = s[i + 3];
            if ((c1 & 0xC0) != 0x80 || (c2 & 0xC0) != 0x80 || (c3 & 0xC0) != 0x80) return 0;
            if (c == 0xF0 && c1 < 0x90) return 0; /* overlong */
            if (c > 0xF4) return 0;
            if (c == 0xF4 && c1 > 0x8F) return 0; /* > U+10FFFF */
            i += 4;
            continue;
        }
        return 0;
    }
    return 1;
}

RAD_API RadValue rad_byte_len(RadValue s) {
    if (s.tag != RV_STR) {
        fprintf(stderr, "Runtime error: byte_len() expects string\n");
        exit(1);
    }
    return rad_make_int(s.as.str.len);
}

RAD_API RadValue rad_byte_at(RadValue s, RadValue idx) {
    if (s.tag != RV_STR || !s.as.str.data) {
        fprintf(stderr, "Runtime error: byte_at() expects string\n");
        exit(1);
    }
    if (idx.tag != RV_INT) {
        fprintf(stderr, "Runtime error: byte_at() expects int index, got %s\n", rad_type_name_for_value(idx));
        exit(1);
    }
    int64_t i = idx.as.i;
    if (i < 0 || i >= s.as.str.len) {
        fprintf(stderr, "Runtime error: out of bounds\n");
        exit(1);
    }
    return rad_make_int((uint8_t)s.as.str.data[i]);
}

RAD_API RadValue rad_substring_bytes(RadValue s, RadValue start, RadValue end) {
    if (s.tag != RV_STR || !s.as.str.data) {
        fprintf(stderr, "Runtime error: substring_bytes() expects string\n");
        exit(1);
    }
    if (start.tag != RV_INT) {
        fprintf(stderr, "Runtime error: substring_bytes() expects int start, got %s\n", rad_type_name_for_value(start));
        exit(1);
    }
    if (end.tag != RV_INT) {
        fprintf(stderr, "Runtime error: substring_bytes() expects int end, got %s\n", rad_type_name_for_value(end));
        exit(1);
    }
    int64_t lo = start.as.i, hi = end.as.i;
    if (lo < 0 || hi < 0) {
        fprintf(stderr, "Runtime error: cannot be negative\n");
        exit(1);
    }
    if (lo > hi) {
        fprintf(stderr, "Runtime error: start cannot be greater than end\n");
        exit(1);
    }
    if (lo > s.as.str.len || hi > s.as.str.len) {
        fprintf(stderr, "Runtime error: out of bounds\n");
        exit(1);
    }
    if (lo == hi) return rad_make_str("");
    int64_t n = hi - lo;
    if (!rad_is_valid_utf8_slice((const unsigned char *)(s.as.str.data + lo), n)) {
        fprintf(stderr, "Runtime error: does not form valid UTF-8\n");
        exit(1);
    }
    RadValue v;
    v.tag = RV_STR;
    v.as.str.data = (char *)rad_intern_string(s.as.str.data + lo, n);
    v.as.str.len = n;
    return v;
}

RAD_API RadValue rad_try_int(RadValue s) {
    if (s.tag == RV_INT) return rad_make_some(s);
    if (s.tag == RV_FLOAT) return rad_make_some(rad_make_int((int64_t)s.as.f));
    if (s.tag == RV_BOOL) return rad_make_some(rad_make_int(s.as.b ? 1 : 0));
    if (s.tag != RV_STR || !s.as.str.data) return rad_make_none();
    char *endptr;
    long long val = strtoll(s.as.str.data, &endptr, 10);
    if (endptr == s.as.str.data || *endptr != '\0') return rad_make_none();
    return rad_make_some(rad_make_int((int64_t)val));
}

RAD_API RadValue rad_try_float(RadValue s) {
    if (s.tag == RV_FLOAT) return rad_make_some(s);
    if (s.tag == RV_INT) return rad_make_some(rad_make_float((double)s.as.i));
    if (s.tag == RV_BOOL) return rad_make_some(rad_make_float(s.as.b ? 1.0 : 0.0));
    if (s.tag != RV_STR || !s.as.str.data) return rad_make_none();
    char *endptr;
    double val = strtod(s.as.str.data, &endptr);
    if (endptr == s.as.str.data || *endptr != '\0') return rad_make_none();
    return rad_make_some(rad_make_float(val));
}

RAD_API RadValue rad_map_or(RadValue opt, RadValue default_val, RadValue fn_unused) {
    if (opt.tag == RV_OPTION_SOME || opt.tag == RV_RESULT_OK) {
        RadValue inner = *opt.as.inner;
        if (fn_unused.tag == RV_FN) return rad_call(fn_unused, &inner, 1);
        return inner;
    }
    if (opt.tag == RV_OPTION_NONE || opt.tag == RV_RESULT_ERR) return default_val;
    if (opt.tag == RV_NIL) return default_val;
    if (fn_unused.tag == RV_FN) return rad_call(fn_unused, &opt, 1);
    return opt;
}

/* ========== File I/O ========== */

RAD_API RadValue rad_read_file(RadValue path) {
    if (path.tag != RV_STR || !path.as.str.data) return rad_make_str("");
    FILE *f = fopen(path.as.str.data, "rb");
    if (!f) {
        fprintf(stderr, "rad runtime: read_file() failed for '%s'\n", path.as.str.data);
        exit(1);
    }
    fseek(f, 0, SEEK_END);
    long sz = ftell(f);
    fseek(f, 0, SEEK_SET);
    char stack_buf[4096];
    char *buf;
    size_t need = (size_t)(sz + 1);
    if (need <= sizeof(stack_buf)) {
        buf = stack_buf;
    } else {
        buf = (char *)malloc(need);
        if (!buf) { fclose(f); fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
    }
    fread(buf, 1, (size_t)sz, f);
    fclose(f);
    buf[sz] = '\0';
    RadValue v;
    v.tag = RV_STR;
    v.as.str.data = (char *)rad_intern_string(buf, (int64_t)sz);
    v.as.str.len = (int64_t)sz;
    if (need > sizeof(stack_buf)) free(buf);
    return v;
}

RAD_API RadValue rad_write_file(RadValue path, RadValue content) {
    if (path.tag != RV_STR || !path.as.str.data) return rad_make_nil();
    FILE *f = fopen(path.as.str.data, "wb");
    if (!f) {
        fprintf(stderr, "rad runtime: write_file() failed for '%s'\n", path.as.str.data);
        exit(1);
    }
    if (content.tag == RV_STR && content.as.str.data)
        fwrite(content.as.str.data, 1, (size_t)content.as.str.len, f);
    fclose(f);
    return rad_make_nil();
}

RAD_API RadValue rad_read_file_bytes(RadValue path) {
    if (path.tag != RV_STR || !path.as.str.data) return rad_make_list();
    FILE *f = fopen(path.as.str.data, "rb");
    if (!f) {
        fprintf(stderr, "rad runtime: read_file_bytes() failed for '%s'\n", path.as.str.data);
        exit(1);
    }
    fseek(f, 0, SEEK_END);
    long sz = ftell(f);
    fseek(f, 0, SEEK_SET);
    unsigned char stack_buf[4096];
    unsigned char *buf;
    size_t need = (size_t)(sz > 0 ? sz : 1);
    if (need <= sizeof(stack_buf)) {
        buf = stack_buf;
    } else {
        buf = (unsigned char *)malloc(need);
        if (!buf) { fclose(f); fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
    }
    size_t read_n = fread(buf, 1, (size_t)sz, f);
    fclose(f);
    RadIntList *xs = rad_list_int_new();
    for (size_t i = 0; i < read_n; i++) {
        rad_list_int_push(xs, (int64_t)buf[i]);
    }
    if (need > sizeof(stack_buf)) free(buf);
    RadValue v; v.tag = RV_LIST_INT; v.as.list_i = xs;
    return v;
}

RAD_API RadValue rad_write_file_bytes(RadValue path, RadValue bytes) {
    if (path.tag != RV_STR || !path.as.str.data) return rad_make_nil();
    FILE *f = fopen(path.as.str.data, "wb");
    if (!f) {
        fprintf(stderr, "rad runtime: write_file_bytes() failed for '%s'\n", path.as.str.data);
        exit(1);
    }
    if (bytes.tag == RV_LIST_INT && bytes.as.list_i) {
        for (int64_t i = 0; i < bytes.as.list_i->len; i++) {
            int64_t x = bytes.as.list_i->data[i];
            if (x < 0) x = 0;
            if (x > 255) x = 255;
            unsigned char b = (unsigned char)x;
            fwrite(&b, 1, 1, f);
        }
    } else if (bytes.tag == RV_LIST && bytes.as.list) {
        for (int64_t i = 0; i < bytes.as.list->len; i++) {
            int64_t x = rad_to_int(bytes.as.list->data[i]);
            if (x < 0) x = 0;
            if (x > 255) x = 255;
            unsigned char b = (unsigned char)x;
            fwrite(&b, 1, 1, f);
        }
    }
    fclose(f);
    return rad_make_nil();
}

RAD_API RadValue rad_append_file(RadValue path, RadValue content) {
    if (path.tag != RV_STR || !path.as.str.data) return rad_make_nil();
    FILE *f = fopen(path.as.str.data, "ab");
    if (!f) {
        fprintf(stderr, "rad runtime: append_file() failed for '%s'\n", path.as.str.data);
        exit(1);
    }
    if (content.tag == RV_STR && content.as.str.data)
        fwrite(content.as.str.data, 1, (size_t)content.as.str.len, f);
    fclose(f);
    return rad_make_nil();
}

RAD_API RadValue rad_file_exists(RadValue path) {
    if (path.tag != RV_STR || !path.as.str.data) return rad_make_bool(false);
    struct stat st;
    return rad_make_bool(stat(path.as.str.data, &st) == 0);
}

RAD_API RadValue rad_remove_file(RadValue path) {
    if (path.tag != RV_STR || !path.as.str.data) return rad_make_nil();
    remove(path.as.str.data);
    return rad_make_nil();
}

RAD_API RadValue rad_create_dir(RadValue path) {
    if (path.tag != RV_STR || !path.as.str.data) return rad_make_nil();
#ifdef _WIN32
    _mkdir(path.as.str.data);
#else
    mkdir(path.as.str.data, 0777);
#endif
    return rad_make_nil();
}

RAD_API RadValue rad_list_dir(RadValue path) {
    RadValue out = rad_make_list();
    if (path.tag != RV_STR || !path.as.str.data) return out;
    DIR *dir = opendir(path.as.str.data);
    if (!dir) return out;
    struct dirent *ent;
    while ((ent = readdir(dir)) != NULL) {
        if (strcmp(ent->d_name, ".") == 0 || strcmp(ent->d_name, "..") == 0) continue;
        out = rad_push(out, rad_make_str(ent->d_name));
    }
    closedir(dir);
    return out;
}

static void rad_remove_dir_recursive_c(const char *path) {
    DIR *dir = opendir(path);
    if (!dir) {
#ifdef _WIN32
        _rmdir(path);
#else
        rmdir(path);
#endif
        return;
    }
    struct dirent *ent;
    while ((ent = readdir(dir)) != NULL) {
        if (strcmp(ent->d_name, ".") == 0 || strcmp(ent->d_name, "..") == 0) continue;
        char child[1024];
        snprintf(child, sizeof(child), "%s/%s", path, ent->d_name);
        DIR *sub = opendir(child);
        if (sub) {
            closedir(sub);
            rad_remove_dir_recursive_c(child);
        } else {
            remove(child);
        }
    }
    closedir(dir);
#ifdef _WIN32
    _rmdir(path);
#else
    rmdir(path);
#endif
}

RAD_API RadValue rad_remove_dir(RadValue path) {
    if (path.tag != RV_STR || !path.as.str.data) return rad_make_nil();
    rad_remove_dir_recursive_c(path.as.str.data);
    return rad_make_nil();
}

/* ========== BitSet (arena-backed) ========== */

RAD_API RadBitSet *rad_bitset_new_impl(void) {
    RadBitSet *bs = (RadBitSet *)rad_arena_alloc(sizeof(RadBitSet));
    bs->words = NULL; bs->capacity = 0;
    return bs;
}

RAD_API void rad_bitset_ensure(RadBitSet *bs, int64_t bit_idx) {
    int64_t word_idx = bit_idx / 64;
    if (word_idx < bs->capacity) return;
    int64_t new_cap = bs->capacity == 0 ? 8 : bs->capacity;
    while (new_cap <= word_idx) new_cap *= 2;
    bs->words = (uint64_t *)rad_arena_realloc(bs->words,
        (size_t)bs->capacity * sizeof(uint64_t), (size_t)new_cap * sizeof(uint64_t));
    bs->capacity = new_cap;
}

RAD_API void rad_bitset_set_impl(RadBitSet *bs, int64_t bit_idx) {
    if (bit_idx < 0 || bit_idx > 100000000) return;
    rad_bitset_ensure(bs, bit_idx);
    bs->words[bit_idx / 64] |= (1ULL << (bit_idx % 64));
}

RAD_API void rad_bitset_clear_impl(RadBitSet *bs, int64_t bit_idx) {
    if (bit_idx < 0 || bit_idx / 64 >= bs->capacity) return;
    bs->words[bit_idx / 64] &= ~(1ULL << (bit_idx % 64));
}

RAD_API bool rad_bitset_has_impl(RadBitSet *bs, int64_t bit_idx) {
    if (bit_idx < 0 || bit_idx / 64 >= bs->capacity) return false;
    return (bs->words[bit_idx / 64] & (1ULL << (bit_idx % 64))) != 0;
}

RAD_API RadValue rad_bitset_new(void) {
    RadValue v; v.tag = RV_BITSET; v.as.bitset = rad_bitset_new_impl(); return v;
}
RAD_API RadValue rad_bitset_set(RadValue bs_val, RadValue idx_val) {
    if (bs_val.tag == RV_BITSET && bs_val.as.bitset) {
        // We mutate in place here because the C runtime uses arena allocation
        // and doesn't have a full RC/GC system for uniqueness yet.
        // It behaves like the optimized inplace mutation.
        rad_bitset_set_impl(bs_val.as.bitset, rad_to_int(idx_val));
        return bs_val;
    }
    return bs_val;
}
RAD_API RadValue rad_bitset_clear(RadValue bs_val, RadValue idx_val) {
    if (bs_val.tag == RV_BITSET && bs_val.as.bitset) {
        rad_bitset_clear_impl(bs_val.as.bitset, rad_to_int(idx_val));
        return bs_val;
    }
    return bs_val;
}
RAD_API RadValue rad_bitset_has(RadValue bs_val, RadValue idx_val) {
    if (bs_val.tag == RV_BITSET && bs_val.as.bitset)
        return rad_make_bool(rad_bitset_has_impl(bs_val.as.bitset, rad_to_int(idx_val)));
    return rad_make_bool(false);
}

/* ========== Index / Misc ========== */

RAD_API RadValue rad_index(RadValue obj, RadValue idx) {
    if (obj.tag == RV_MAP && obj.as.map) {
        for (int64_t i = 0; i < obj.as.map->len; i++) {
            if (rad_is_truthy(rad_eq(obj.as.map->keys[i], idx))) return obj.as.map->vals[i];
        }
        return rad_make_nil();
    }
    if (idx.tag != RV_INT) {
        fprintf(stderr, "Runtime error: index must be int, got %s\n", rad_type_name_for_value(idx));
        exit(1);
    }
    int64_t i = idx.as.i;
    if (i < 0) {
        fprintf(stderr, "Runtime error: Negative index\n");
        exit(1);
    }
    if (obj.tag == RV_TUPLE && obj.as.tuple) {
        if (i >= obj.as.tuple->len) {
            fprintf(stderr, "Runtime error: Tuple index %lld out of bounds\n", (long long)i);
            exit(1);
        }
        return obj.as.tuple->data[i];
    }
    if (obj.tag == RV_LIST_INT && obj.as.list_i) {
        if (i >= obj.as.list_i->len) {
            fprintf(stderr, "Runtime error: List index %lld out of bounds\n", (long long)i);
            exit(1);
        }
        return rad_make_int(obj.as.list_i->data[i]);
    }
    if (obj.tag == RV_LIST && obj.as.list) {
        if (i >= obj.as.list->len) {
            fprintf(stderr, "Runtime error: List index %lld out of bounds\n", (long long)i);
            exit(1);
        }
        return obj.as.list->data[i];
    }
    if (obj.tag == RV_STR && obj.as.str.data) {
        if (i >= obj.as.str.len) {
            fprintf(stderr, "Runtime error: out of bounds\n");
            exit(1);
        }
        return rad_make_int((int64_t)(unsigned char)obj.as.str.data[i]);
    }
    return rad_make_nil();
}

RAD_API RadValue rad_slice(RadValue obj, RadValue start, RadValue end) {
    int64_t lo = rad_to_int(start), hi = rad_to_int(end);
    if (obj.tag == RV_STR && obj.as.str.data) {
        return rad_substring_bytes(obj, rad_make_int(lo), rad_make_int(hi));
    }
    int64_t n = 0;
    if (obj.tag == RV_LIST && obj.as.list) n = obj.as.list->len;
    if (obj.tag == RV_LIST_INT && obj.as.list_i) n = obj.as.list_i->len;
    if (lo < 0) lo = 0;
    if (hi < 0) hi = 0;
    if (lo > n) lo = n;
    if (hi > n) hi = n;
    if (hi < lo) hi = lo;
    RadValue out = rad_make_list();
    for (int64_t i = lo; i < hi; i++) {
        out = rad_push(out, rad_index(obj, rad_make_int(i)));
    }
    return out;
}

static int rad_value_cmp(RadValue a, RadValue b) {
    if ((a.tag == RV_INT || a.tag == RV_FLOAT) && (b.tag == RV_INT || b.tag == RV_FLOAT)) {
        double av = rad_cmp_num(a), bv = rad_cmp_num(b);
        if (av < bv) return -1;
        if (av > bv) return 1;
        return 0;
    }
    if (a.tag == RV_STR && b.tag == RV_STR) {
        return strcmp(a.as.str.data ? a.as.str.data : "", b.as.str.data ? b.as.str.data : "");
    }
    RadValue sa = str(a), sb = str(b);
    return strcmp(sa.as.str.data ? sa.as.str.data : "", sb.as.str.data ? sb.as.str.data : "");
}

RAD_API RadValue rad_sort(RadValue lst) {
    int64_t n = 0;
    if (lst.tag == RV_LIST && lst.as.list) n = lst.as.list->len;
    if (lst.tag == RV_LIST_INT && lst.as.list_i) n = lst.as.list_i->len;
    if (n <= 0) return rad_make_list();
    RadValue out = rad_make_list();
    for (int64_t i = 0; i < n; i++) {
        out = rad_push(out, rad_index(lst, rad_make_int(i)));
    }
    if (out.tag != RV_LIST || !out.as.list) return out;
    for (int64_t i = 1; i < out.as.list->len; i++) {
        RadValue key = out.as.list->data[i];
        int64_t j = i - 1;
        while (j >= 0 && rad_value_cmp(out.as.list->data[j], key) > 0) {
            out.as.list->data[j + 1] = out.as.list->data[j];
            j--;
        }
        out.as.list->data[j + 1] = key;
    }
    return out;
}

RAD_API RadValue rad_sys_args(void) {
    RadValue v = rad_make_list();
    for (int i = 0; i < g_argc; i++)
        rad_list_push(v.as.list, rad_make_str(g_argv[i]));
    return v;
}

static void rad_event_queue_reserve_one(void) {
    if (rad_event_queue_len + 1 <= rad_event_queue_cap) return;
    int64_t new_cap = rad_event_queue_cap == 0 ? 16 : rad_event_queue_cap * 2;
    rad_event_queue = (RadPendingEvent *)rad_arena_realloc(
        rad_event_queue,
        (size_t)rad_event_queue_cap * sizeof(RadPendingEvent),
        (size_t)new_cap * sizeof(RadPendingEvent));
    rad_event_queue_cap = new_cap;
}

static void rad_event_handlers_reserve_one(void) {
    if (rad_event_handlers_len + 1 <= rad_event_handlers_cap) return;
    int64_t new_cap = rad_event_handlers_cap == 0 ? 16 : rad_event_handlers_cap * 2;
    rad_event_handlers = (RadEventHandlerSlot *)rad_arena_realloc(
        rad_event_handlers,
        (size_t)rad_event_handlers_cap * sizeof(RadEventHandlerSlot),
        (size_t)new_cap * sizeof(RadEventHandlerSlot));
    rad_event_handlers_cap = new_cap;
}

RAD_API RadValue rad_event_on(const char *event_name, RadEventHandlerFn handler, int once) {
    if (!event_name || !handler) return rad_make_nil();
    rad_event_handlers_reserve_one();
    rad_event_handlers[rad_event_handlers_len].event_name = event_name;
    rad_event_handlers[rad_event_handlers_len].fn = handler;
    rad_event_handlers[rad_event_handlers_len].once = once ? 1 : 0;
    rad_event_handlers[rad_event_handlers_len].active = 1;
    rad_event_handlers_len++;
    return rad_make_nil();
}

RAD_API RadValue rad_event_emit(const char *event_name, RadValue payload) {
    if (!event_name) return rad_make_nil();
    if (rad_simulation_depth > 0) return rad_make_nil();
    rad_event_queue_reserve_one();
    rad_event_queue[rad_event_queue_len].event_name = event_name;
    rad_event_queue[rad_event_queue_len].payload = payload;
    rad_event_queue_len++;
    return rad_make_nil();
}

RAD_API RadValue rad_flush_events(void) {
    if (rad_event_queue_len == 0) return rad_make_nil();
    int64_t batch_len = rad_event_queue_len;
    RadPendingEvent stack_batch[32];
    RadPendingEvent *batch;
    size_t need = (size_t)batch_len * sizeof(RadPendingEvent);
    if ((size_t)batch_len <= sizeof(stack_batch) / sizeof(stack_batch[0])) {
        batch = stack_batch;
    } else {
        batch = (RadPendingEvent *)malloc(need);
        if (!batch) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
    }
    memcpy(batch, rad_event_queue, need);
    rad_event_queue_len = 0;

    for (int64_t i = 0; i < batch_len; i++) {
        const char *event_name = batch[i].event_name;
        RadValue payload = batch[i].payload;
        for (int64_t h = 0; h < rad_event_handlers_len; h++) {
            RadEventHandlerSlot *slot = &rad_event_handlers[h];
            if (!slot->active) continue;
            if (!slot->event_name || strcmp(slot->event_name, event_name) != 0) continue;
            (void)slot->fn(payload);
            if (slot->once) slot->active = 0;
        }
    }
    if ((size_t)batch_len > sizeof(stack_batch) / sizeof(stack_batch[0])) free(batch);
    return rad_make_nil();
}

RAD_API void rad_task_context_push(void) { rad_task_context_depth++; }
RAD_API void rad_task_context_pop(void) { if (rad_task_context_depth > 0) rad_task_context_depth--; }

RAD_API RadValue rad_task_from_value(RadValue v) {
    RadValue task = rad_make_map();
    task = rad_map_set(task, rad_make_str("__rad_task"), rad_make_bool(true));
    task = rad_map_set(task, rad_make_str("value"), v);
    return task;
}

RAD_API RadValue rad_await_task(RadValue task) {
    if (task.tag != RV_MAP || !task.as.map) {
        fprintf(stderr, "Runtime error: `await` expects a task value\n");
        exit(1);
    }
    RadValue marker = rad_map_get(task, rad_make_str("__rad_task"));
    if (!rad_is_truthy(marker)) {
        fprintf(stderr, "Runtime error: `await` expects a task value\n");
        exit(1);
    }
    return rad_map_get(task, rad_make_str("value"));
}

/* ========== Buffer (arena-backed growable string) ========== */

RAD_API RadValue rad_buffer_new(void) {
    RadBuffer *b = (RadBuffer *)rad_arena_alloc(sizeof(RadBuffer));
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
    RadValue v;
    v.tag = RV_BUFFER;
    v.as.buffer = b;
    return v;
}

RAD_API RadValue rad_buffer_append(RadValue buf_val, RadValue str_val) {
    if (buf_val.tag != RV_BUFFER || !buf_val.as.buffer) return buf_val;
    RadBuffer *b = buf_val.as.buffer;
    const char *s = NULL;
    int64_t slen = 0;
    if (str_val.tag == RV_STR && str_val.as.str.data) {
        s = str_val.as.str.data;
        slen = str_val.as.str.len;
    }
    if (!s || slen == 0) return buf_val;
    int64_t need = b->len + slen;
    if (need > b->cap) {
        int64_t new_cap = b->cap == 0 ? 4096 : b->cap;
        while (new_cap < need) new_cap *= 2;
        b->data = (char *)rad_arena_realloc(b->data, (size_t)b->cap, (size_t)new_cap);
        b->cap = new_cap;
    }
    memcpy(b->data + b->len, s, (size_t)slen);
    b->len += slen;
    return buf_val;
}

RAD_API RadValue rad_buffer_to_str(RadValue buf_val) {
    if (buf_val.tag != RV_BUFFER || !buf_val.as.buffer) return rad_make_str("");
    RadBuffer *b = buf_val.as.buffer;
    if (b->len == 0) return rad_make_str("");
    RadValue v;
    v.tag = RV_STR;
    v.as.str.data = (char *)rad_intern_string(b->data, b->len);
    v.as.str.len = b->len;
    return v;
}

/* ========== World Forking (Speculative Execution) ========== */

RAD_API RadWorldFork *rad_world_fork_snapshot(void) {
    RadWorldFork *snap = (RadWorldFork *)rad_arena_alloc(sizeof(RadWorldFork));
    snap->num_components = rad_next_component_id;
    snap->next_entity = rad_next_entity;
    snap->mask_cap = rad_mask_cap;
    snap->mask_words = rad_mask_words;
    snap->entity_names_init = rad_entity_names_init;

    snap->columns = (RadRefColumn **)rad_arena_alloc((size_t)snap->num_components * sizeof(RadRefColumn *));
    for (int64_t i = 0; i < snap->num_components; i++) {
        snap->columns[i] = rad_components[i].column;
        if (snap->columns[i]) {
            rad_refcol_retain(snap->columns[i]);
        }
    }

    snap->entity_masks_ref = rad_masks_ref;
    if (snap->entity_masks_ref) {
        rad_ref_u64_retain(snap->entity_masks_ref);
    }
    snap->entity_alive_ref = rad_alive_ref;
    if (snap->entity_alive_ref) {
        rad_ref_u64_retain(snap->entity_alive_ref);
    }

    if (rad_entity_names_init) {
        snap->entity_names = rad_value_deep_copy(rad_entity_names);
        snap->entity_id_to_name = rad_value_deep_copy(rad_entity_id_to_name);
    } else {
        snap->entity_names = rad_make_nil();
        snap->entity_id_to_name = rad_make_nil();
    }

    snap->free_count = rad_free_count;
    snap->free_cap = rad_free_cap;
    if (rad_free_count > 0 && rad_free_ids) {
        snap->free_ids = (int64_t *)rad_arena_alloc((size_t)rad_free_count * sizeof(int64_t));
        memcpy(snap->free_ids, rad_free_ids, (size_t)rad_free_count * sizeof(int64_t));
    } else {
        snap->free_ids = NULL;
    }

    return snap;
}

RAD_API void rad_world_fork_restore(RadWorldFork *snap) {
    int64_t old_num_components = rad_next_component_id;
    for (int64_t i = 0; i < old_num_components && i < RAD_MAX_COMPONENTS; i++) {
        if (rad_components[i].column) {
            rad_refcol_release(rad_components[i].column);
            rad_components[i].column = NULL;
        }
    }
    for (int64_t i = 0; i < snap->num_components; i++) {
        rad_components[i].column = snap->columns[i];
        if (rad_components[i].column) {
            rad_refcol_retain(rad_components[i].column);
        }
    }
    rad_next_component_id = snap->num_components;

    rad_next_entity = snap->next_entity;

    rad_free_count = snap->free_count;
    if (snap->free_count > 0 && snap->free_ids) {
        int64_t need_cap = snap->free_cap > 0 ? snap->free_cap : snap->free_count;
        if (rad_free_cap < need_cap) {
            rad_free_ids = (int64_t *)rad_arena_realloc(rad_free_ids,
                (size_t)rad_free_cap * sizeof(int64_t), (size_t)need_cap * sizeof(int64_t));
            rad_free_cap = need_cap;
        }
        memcpy(rad_free_ids, snap->free_ids, (size_t)snap->free_count * sizeof(int64_t));
    } else {
        rad_free_count = 0;
    }

    RadRefU64Array *old_masks = rad_masks_ref;
    RadRefU64Array *old_alive = rad_alive_ref;
    rad_attach_masks_ref(snap->entity_masks_ref);
    rad_attach_alive_ref(snap->entity_alive_ref);
    if (rad_masks_ref) rad_ref_u64_retain(rad_masks_ref);
    if (rad_alive_ref) rad_ref_u64_retain(rad_alive_ref);
    rad_ref_u64_release(old_masks);
    rad_ref_u64_release(old_alive);

    rad_mask_cap = snap->mask_cap;
    rad_mask_words = snap->mask_words;

    rad_entity_names_init = snap->entity_names_init;
    if (rad_entity_names_init) {
        rad_entity_names = rad_value_deep_copy(snap->entity_names);
        rad_entity_id_to_name = rad_value_deep_copy(snap->entity_id_to_name);
    } else {
        rad_entity_names = rad_make_nil();
        rad_entity_id_to_name = rad_make_nil();
    }
}

RAD_API RadValue rad_fork(void) {
    RadValue v;
    v.tag = RV_WORLD_FORK;
    v.as.world_fork = rad_world_fork_snapshot();
    return v;
}

#ifdef RAD_SEPARATE_COMPILATION
#if defined(__GNUC__) || defined(__clang__)
__attribute__((weak)) void rad_dispatch_system(RadValue sys_name) {
    fprintf(stderr, "rad runtime: rad_dispatch_system not linked\n");
    exit(1);
}
#endif
#endif

RAD_API RadValue rad_simulate(RadValue fork_val, RadValue systems, RadValue ticks) {
    if (fork_val.tag != RV_WORLD_FORK || !fork_val.as.world_fork) {
        fprintf(stderr, "rad runtime: simulate() first argument must be a world_fork\n");
        exit(1);
    }
    if (systems.tag != RV_LIST || !systems.as.list) {
        fprintf(stderr, "rad runtime: simulate() second argument must be a list of system names\n");
        exit(1);
    }
    if (ticks.tag != RV_INT) {
        fprintf(stderr, "rad runtime: simulate() third argument must be an integer\n");
        exit(1);
    }
    int64_t num_ticks = ticks.as.i;
    if (num_ticks < 0) {
        fprintf(stderr, "rad runtime: simulate() tick count must be non-negative\n");
        exit(1);
    }

    RadPendingEvent *saved_queue = rad_event_queue;
    int64_t saved_queue_len = rad_event_queue_len;
    int64_t saved_queue_cap = rad_event_queue_cap;
    rad_event_queue = NULL;
    rad_event_queue_len = 0;
    rad_event_queue_cap = 0;

    RadWorldFork *saved_world = rad_world_fork_snapshot();
    rad_simulation_depth++;
    rad_world_fork_restore(fork_val.as.world_fork);

    for (int64_t i = 0; i < num_ticks; i++) {
        for (int64_t j = 0; j < systems.as.list->len; j++) {
            rad_dispatch_system(systems.as.list->data[j]);
        }
        rad_flush_events();
    }

    RadWorldFork *new_fork = rad_world_fork_snapshot();
    rad_world_fork_restore(saved_world);
    rad_simulation_depth--;

    rad_event_queue = saved_queue;
    rad_event_queue_len = saved_queue_len;
    rad_event_queue_cap = saved_queue_cap;

    RadValue v;
    v.tag = RV_WORLD_FORK;
    v.as.world_fork = new_fork;
    return v;
}

RAD_API RadValue rad_commit(RadValue fork_val) {
    if (fork_val.tag != RV_WORLD_FORK || !fork_val.as.world_fork) {
        fprintf(stderr, "rad runtime: commit() argument must be a world_fork\n");
        exit(1);
    }
    rad_world_fork_restore(fork_val.as.world_fork);
    rad_event_queue_len = 0;
    return rad_make_nil();
}

RAD_API RadValue rad_peek(RadValue fork_val, RadValue ent, int64_t comp_id) {
    if (fork_val.tag != RV_WORLD_FORK || !fork_val.as.world_fork) {
        fprintf(stderr, "rad runtime: peek() first argument must be a world_fork\n");
        exit(1);
    }
    if (ent.tag != RV_ENTITY) {
        return rad_make_none();
    }
    RadWorldFork *snap = fork_val.as.world_fork;
    int64_t eid = ent.as.entity_id;
    if (comp_id >= snap->num_components || comp_id < 0) {
        return rad_make_none();
    }
    if (eid >= snap->mask_cap || eid < 0) {
        return rad_make_none();
    }
    if (!snap->entity_masks_ref || !snap->entity_masks_ref->data) {
        return rad_make_none();
    }
    if (!(snap->entity_masks_ref->data[eid * snap->mask_words + (comp_id >> 6)] & (1ULL << (comp_id & 63)))) {
        return rad_make_none();
    }
    RadRefColumn *col = snap->columns[comp_id];
    if (!col || !col->data || col->capacity <= eid) {
        return rad_make_none();
    }
    return rad_make_some(rad_value_deep_copy_from_fork(col->data[eid], snap));
}

#ifdef _WIN32
#include <windows.h>
RAD_API RadValue rad_clock(void) {
    static LARGE_INTEGER freq = {0};
    LARGE_INTEGER now;
    if (freq.QuadPart == 0) QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&now);
    return rad_make_float((double)now.QuadPart / (double)freq.QuadPart);
}
#else
#include <time.h>
RAD_API RadValue rad_clock(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return rad_make_float((double)ts.tv_sec + (double)ts.tv_nsec * 1e-9);
}
#endif

/* ========== List builtins ========== */

RAD_API RadValue rad_pop(RadValue lst) {
    if (lst.tag == RV_LIST_INT && lst.as.list_i && lst.as.list_i->len > 0) {
        return rad_make_int(lst.as.list_i->data[lst.as.list_i->len - 1]);
    }
    if (lst.tag == RV_LIST && lst.as.list && lst.as.list->len > 0) {
        return lst.as.list->data[lst.as.list->len - 1];
    }
    if (rad_task_context_depth > 0) {
        fprintf(stderr, "Runtime error: Task pop() on empty list\n");
    } else {
        fprintf(stderr, "Runtime error: pop() on empty list\n");
    }
    exit(1);
}

RAD_API RadValue rad_pop_last(RadValue lst) {
    if (lst.tag == RV_LIST_INT && lst.as.list_i && lst.as.list_i->len > 0) {
        return rad_make_int(lst.as.list_i->data[lst.as.list_i->len - 1]);
    }
    if (lst.tag == RV_LIST && lst.as.list && lst.as.list->len > 0) {
        return lst.as.list->data[lst.as.list->len - 1];
    }
    fprintf(stderr, "Runtime error: pop_last() on empty list\n");
    exit(1);
}

RAD_API RadValue rad_drop_last(RadValue lst) {
    if (lst.tag == RV_LIST_INT && lst.as.list_i) {
        if (lst.as.list_i->len == 0) {
            fprintf(stderr, "Runtime error: drop_last() on empty list\n");
            exit(1);
        }
        RadIntList *xs = rad_list_int_new();
        for (int64_t i = 0; i + 1 < lst.as.list_i->len; i++)
            rad_list_int_push(xs, lst.as.list_i->data[i]);
        RadValue v; v.tag = RV_LIST_INT; v.as.list_i = xs; return v;
    }
    if (lst.tag == RV_LIST && lst.as.list) {
        if (lst.as.list->len == 0) {
            fprintf(stderr, "Runtime error: drop_last() on empty list\n");
            exit(1);
        }
        RadList *xs = rad_list_new();
        for (int64_t i = 0; i + 1 < lst.as.list->len; i++)
            rad_list_push(xs, lst.as.list->data[i]);
        RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
    }
    fprintf(stderr, "Runtime error: drop_last() on empty list\n");
    exit(1);
}

RAD_API RadValue rad_reverse(RadValue lst) {
    if (lst.tag == RV_STR && lst.as.str.data) {
        int64_t n = lst.as.str.len;
        char stack_buf[256];
        char *buf;
        size_t need = (size_t)n + 1;
        if (need <= sizeof(stack_buf)) {
            buf = stack_buf;
        } else {
            buf = (char *)malloc(need);
            if (!buf) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
        }
        for (int64_t i = 0; i < n; i++) {
            buf[i] = lst.as.str.data[n - 1 - i];
        }
        buf[n] = '\0';
        RadValue v;
        v.tag = RV_STR;
        v.as.str.data = (char *)rad_intern_string(buf, n);
        v.as.str.len = n;
        if (need > sizeof(stack_buf)) free(buf);
        return v;
    }
    if (lst.tag == RV_LIST_INT && lst.as.list_i) {
        RadIntList *xs = rad_list_int_new();
        for (int64_t i = lst.as.list_i->len - 1; i >= 0; i--)
            rad_list_int_push(xs, lst.as.list_i->data[i]);
        RadValue v; v.tag = RV_LIST_INT; v.as.list_i = xs; return v;
    }
    if (lst.tag == RV_LIST && lst.as.list) {
        RadList *xs = rad_list_new();
        for (int64_t i = lst.as.list->len - 1; i >= 0; i--)
            rad_list_push(xs, lst.as.list->data[i]);
        RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
    }
    return rad_make_list();
}

RAD_API RadValue rad_append(RadValue a, RadValue b) {
    RadList *xs = rad_list_new();
    if (a.tag == RV_LIST && a.as.list)
        for (int64_t i = 0; i < a.as.list->len; i++) rad_list_push(xs, a.as.list->data[i]);
    if (a.tag == RV_LIST_INT && a.as.list_i)
        for (int64_t i = 0; i < a.as.list_i->len; i++) rad_list_push(xs, rad_make_int(a.as.list_i->data[i]));
    if (b.tag == RV_LIST && b.as.list)
        for (int64_t i = 0; i < b.as.list->len; i++) rad_list_push(xs, b.as.list->data[i]);
    if (b.tag == RV_LIST_INT && b.as.list_i)
        for (int64_t i = 0; i < b.as.list_i->len; i++) rad_list_push(xs, rad_make_int(b.as.list_i->data[i]));
    RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
}

RAD_API RadValue rad_zip(RadValue a, RadValue b) {
    RadList *xs = rad_list_new();
    int64_t alen = 0, blen = 0;
    if (a.tag == RV_LIST && a.as.list) alen = a.as.list->len;
    if (a.tag == RV_LIST_INT && a.as.list_i) alen = a.as.list_i->len;
    if (b.tag == RV_LIST && b.as.list) blen = b.as.list->len;
    if (b.tag == RV_LIST_INT && b.as.list_i) blen = b.as.list_i->len;
    int64_t n = alen < blen ? alen : blen;
    for (int64_t i = 0; i < n; i++) {
        RadList *pair = rad_list_new();
        rad_list_push(pair, rad_index(a, rad_make_int(i)));
        rad_list_push(pair, rad_index(b, rad_make_int(i)));
        RadValue p; p.tag = RV_LIST; p.as.list = pair;
        rad_list_push(xs, p);
    }
    RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
}

RAD_API RadValue rad_enumerate(RadValue list) {
    RadList *xs = rad_list_new();
    int64_t n = 0;
    if (list.tag == RV_LIST && list.as.list) n = list.as.list->len;
    else if (list.tag == RV_LIST_INT && list.as.list_i) n = list.as.list_i->len;
    for (int64_t i = 0; i < n; i++) {
        RadList *pair = rad_list_new();
        rad_list_push(pair, rad_make_int(i));
        rad_list_push(pair, rad_index(list, rad_make_int(i)));
        RadValue p; p.tag = RV_LIST; p.as.list = pair;
        rad_list_push(xs, p);
    }
    RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
}

/* ========== Type / option builtins ========== */

RAD_API RadValue rad_typeof(RadValue v) {
    switch (v.tag) {
    case RV_NIL: return rad_make_str("nil");
    case RV_INT: return rad_make_str("int");
    case RV_FLOAT: return rad_make_str("float");
    case RV_BOOL: return rad_make_str("bool");
    case RV_STR: return rad_make_str("str");
    case RV_LIST_INT: case RV_LIST: return rad_make_str("list");
    case RV_TUPLE: return rad_make_str("tuple");
    case RV_MAP: return rad_make_str("map");
    case RV_STRUCT: return rad_make_str("struct");
    case RV_OPTION_SOME: case RV_OPTION_NONE: return rad_make_str("Option");
    case RV_RESULT_OK: case RV_RESULT_ERR: return rad_make_str("Result");
    case RV_ENTITY: return rad_make_str("entity");
    case RV_FN: return rad_make_str("fn");
    case RV_BITSET: return rad_make_str("bitset");
    case RV_WORLD_FORK: return rad_make_str("world_fork");
    case RV_BUFFER: return rad_make_str("buffer");
    default: return rad_make_str("unknown");
    }
}

RAD_API RadValue rad_unwrap(RadValue v) {
    if (v.tag == RV_OPTION_SOME || v.tag == RV_RESULT_OK) return *v.as.inner;
    if (v.tag == RV_OPTION_NONE) {
        fprintf(stderr, "Runtime error: unwrap() called on Option::None\n");
        exit(1);
    }
    if (v.tag == RV_RESULT_ERR) {
        RadValue msg = *v.as.inner;
        fprintf(stderr, "Runtime error: unwrap() called on Result::Err");
        if (msg.tag == RV_STR && msg.as.str.data) {
            fprintf(stderr, ": %s", msg.as.str.data);
        }
        fprintf(stderr, "\n");
        exit(1);
    }
    if (v.tag == RV_NIL) { fprintf(stderr, "Runtime error: unwrap() called on nil\n"); exit(1); }
    return v;
}

RAD_API RadValue rad_unwrap_or(RadValue v, RadValue def) {
    if (v.tag == RV_OPTION_SOME || v.tag == RV_RESULT_OK) return *v.as.inner;
    if (v.tag == RV_OPTION_NONE || v.tag == RV_RESULT_ERR) return def;
    return v.tag == RV_NIL ? def : v;
}

RAD_API RadValue rad_is_some(RadValue v) {
    if (v.tag == RV_OPTION_SOME || v.tag == RV_RESULT_OK) return rad_make_bool(true);
    if (v.tag == RV_OPTION_NONE || v.tag == RV_RESULT_ERR) return rad_make_bool(false);
    return rad_make_bool(v.tag != RV_NIL);
}
RAD_API RadValue rad_is_none(RadValue v) {
    if (v.tag == RV_OPTION_NONE || v.tag == RV_RESULT_ERR) return rad_make_bool(true);
    if (v.tag == RV_OPTION_SOME || v.tag == RV_RESULT_OK) return rad_make_bool(false);
    return rad_make_bool(v.tag == RV_NIL);
}

RAD_API RadValue rad_expect(RadValue v, RadValue msg) {
    int is_none = (v.tag == RV_NIL || v.tag == RV_OPTION_NONE);
    if (is_none) {
        fprintf(stderr, "Runtime error: expect() failed: ");
        if (msg.tag == RV_STR && msg.as.str.data) fprintf(stderr, "%s", msg.as.str.data);
        fprintf(stderr, "\n"); exit(1);
    }
    return v;
}

/* ========== String builtins ========== */

RAD_API RadValue rad_chr(RadValue v) {
    char buf[2] = { (char)rad_to_int(v), 0 };
    return rad_make_str(buf);
}

RAD_API RadValue rad_ord(RadValue v) {
    if (v.tag == RV_STR && v.as.str.data && v.as.str.len > 0)
        return rad_make_int((unsigned char)v.as.str.data[0]);
    return rad_make_int(0);
}

RAD_API RadValue rad_split(RadValue s, RadValue sep) {
    RadList *xs = rad_list_new();
    if (s.tag != RV_STR || !s.as.str.data) {
        RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
    }
    const char *sd = s.as.str.data;
    int64_t slen = s.as.str.len;
    if (sep.tag != RV_STR || !sep.as.str.data || sep.as.str.len == 0) {
        for (int64_t i = 0; i < slen; i++) {
            char buf[2] = { sd[i], 0 };
            rad_list_push(xs, rad_make_str(buf));
        }
        RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
    }
    const char *sepd = sep.as.str.data;
    int64_t seplen = sep.as.str.len;
    int64_t start = 0;
    for (int64_t i = 0; i <= slen - seplen; i++) {
        if (memcmp(sd + i, sepd, seplen) == 0) {
            char *part = (char *)rad_arena_alloc(i - start + 1);
            memcpy(part, sd + start, i - start);
            part[i - start] = 0;
            RadValue pv; pv.tag = RV_STR; pv.as.str.data = part; pv.as.str.len = i - start;
            rad_list_push(xs, pv);
            start = i + seplen;
            i = start - 1;
        }
    }
    char *part = (char *)rad_arena_alloc(slen - start + 1);
    memcpy(part, sd + start, slen - start);
    part[slen - start] = 0;
    RadValue pv; pv.tag = RV_STR; pv.as.str.data = part; pv.as.str.len = slen - start;
    rad_list_push(xs, pv);
    RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
}

RAD_API RadValue rad_join(RadValue lst, RadValue sep) {
    const char *sepd = (sep.tag == RV_STR && sep.as.str.data) ? sep.as.str.data : "";
    int64_t seplen = (sep.tag == RV_STR) ? sep.as.str.len : 0;
    char *data = NULL;
    int64_t len = 0, cap = 0;
    int64_t n = 0;
    if (lst.tag == RV_LIST && lst.as.list) n = lst.as.list->len;
    if (lst.tag == RV_LIST_INT && lst.as.list_i) n = lst.as.list_i->len;
    for (int64_t i = 0; i < n; i++) {
        if (i > 0 && seplen > 0) {
            int64_t need = len + seplen;
            if (need > cap) { int64_t nc = cap == 0 ? 256 : cap; while (nc < need) nc *= 2; data = (char *)realloc(data, (size_t)nc); cap = nc; }
            memcpy(data + len, sepd, (size_t)seplen);
            len += seplen;
        }
        RadValue elem = rad_index(lst, rad_make_int(i));
        RadValue sv = str(elem);
        const char *sd = (sv.tag == RV_STR && sv.as.str.data) ? sv.as.str.data : "";
        int64_t sl = (sv.tag == RV_STR) ? sv.as.str.len : 0;
        if (sl > 0) {
            int64_t need = len + sl;
            if (need > cap) { int64_t nc = cap == 0 ? 256 : cap; while (nc < need) nc *= 2; data = (char *)realloc(data, (size_t)nc); cap = nc; }
            memcpy(data + len, sd, (size_t)sl);
            len += sl;
        }
    }
    if (len == 0) { free(data); return rad_make_str(""); }
    RadValue v; v.tag = RV_STR;
    v.as.str.data = (char *)rad_intern_string(data, len);
    v.as.str.len = len;
    free(data);
    return v;
}

RAD_API RadValue rad_chars(RadValue s) {
    RadList *xs = rad_list_new();
    if (s.tag == RV_STR && s.as.str.data) {
        for (int64_t i = 0; i < s.as.str.len; i++) {
            char buf[2] = { s.as.str.data[i], 0 };
            rad_list_push(xs, rad_make_str(buf));
        }
    }
    RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
}

RAD_API RadValue rad_trim(RadValue s) {
    if (s.tag != RV_STR || !s.as.str.data || s.as.str.len == 0) return rad_make_str("");
    const char *d = s.as.str.data;
    int64_t lo = 0, hi = s.as.str.len;
    while (lo < hi && (d[lo] == ' ' || d[lo] == '\t' || d[lo] == '\n' || d[lo] == '\r')) lo++;
    while (hi > lo && (d[hi-1] == ' ' || d[hi-1] == '\t' || d[hi-1] == '\n' || d[hi-1] == '\r')) hi--;
    char stack_buf[256];
    char *out;
    size_t need = (size_t)(hi - lo + 1);
    if (need <= sizeof(stack_buf)) {
        out = stack_buf;
    } else {
        out = (char *)malloc(need);
        if (!out) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
    }
    memcpy(out, d + lo, hi - lo); out[hi - lo] = 0;
    RadValue v; v.tag = RV_STR;
    v.as.str.data = (char *)rad_intern_string(out, hi - lo);
    v.as.str.len = hi - lo;
    if (need > sizeof(stack_buf)) free(out);
    return v;
}

RAD_API RadValue rad_starts_with(RadValue s, RadValue prefix) {
    if (s.tag != RV_STR || prefix.tag != RV_STR) return rad_make_bool(false);
    if (!s.as.str.data || !prefix.as.str.data) return rad_make_bool(false);
    if (prefix.as.str.len > s.as.str.len) return rad_make_bool(false);
    return rad_make_bool(memcmp(s.as.str.data, prefix.as.str.data, prefix.as.str.len) == 0);
}

RAD_API RadValue rad_ends_with(RadValue s, RadValue suffix) {
    if (s.tag != RV_STR || suffix.tag != RV_STR) return rad_make_bool(false);
    if (!s.as.str.data || !suffix.as.str.data) return rad_make_bool(false);
    if (suffix.as.str.len > s.as.str.len) return rad_make_bool(false);
    int64_t off = s.as.str.len - suffix.as.str.len;
    return rad_make_bool(memcmp(s.as.str.data + off, suffix.as.str.data, suffix.as.str.len) == 0);
}

RAD_API RadValue rad_to_upper(RadValue s) {
    if (s.tag != RV_STR || !s.as.str.data) return rad_make_str("");
    char stack_buf[256];
    char *out;
    size_t need = (size_t)(s.as.str.len + 1);
    if (need <= sizeof(stack_buf)) {
        out = stack_buf;
    } else {
        out = (char *)malloc(need);
        if (!out) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
    }
    for (int64_t i = 0; i < s.as.str.len; i++)
        out[i] = (s.as.str.data[i] >= 'a' && s.as.str.data[i] <= 'z') ? s.as.str.data[i] - 32 : s.as.str.data[i];
    out[s.as.str.len] = 0;
    RadValue v; v.tag = RV_STR;
    v.as.str.data = (char *)rad_intern_string(out, s.as.str.len);
    v.as.str.len = s.as.str.len;
    if (need > sizeof(stack_buf)) free(out);
    return v;
}

RAD_API RadValue rad_to_lower(RadValue s) {
    if (s.tag != RV_STR || !s.as.str.data) return rad_make_str("");
    char stack_buf[256];
    char *out;
    size_t need = (size_t)(s.as.str.len + 1);
    if (need <= sizeof(stack_buf)) {
        out = stack_buf;
    } else {
        out = (char *)malloc(need);
        if (!out) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
    }
    for (int64_t i = 0; i < s.as.str.len; i++)
        out[i] = (s.as.str.data[i] >= 'A' && s.as.str.data[i] <= 'Z') ? s.as.str.data[i] + 32 : s.as.str.data[i];
    out[s.as.str.len] = 0;
    RadValue v; v.tag = RV_STR;
    v.as.str.data = (char *)rad_intern_string(out, s.as.str.len);
    v.as.str.len = s.as.str.len;
    if (need > sizeof(stack_buf)) free(out);
    return v;
}

RAD_API RadValue rad_string_repeat(RadValue s, RadValue n) {
    if (s.tag != RV_STR || !s.as.str.data) return rad_make_str("");
    int64_t count = rad_to_int(n);
    if (count <= 0) return rad_make_str("");
    int64_t slen = s.as.str.len;
    int64_t total = slen * count;
    char stack_buf[256];
    char *out;
    size_t need = (size_t)(total + 1);
    if (need <= sizeof(stack_buf)) {
        out = stack_buf;
    } else {
        out = (char *)malloc(need);
        if (!out) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
    }
    for (int64_t i = 0; i < count; i++) memcpy(out + i * slen, s.as.str.data, slen);
    out[total] = 0;
    RadValue v; v.tag = RV_STR;
    v.as.str.data = (char *)rad_intern_string(out, total);
    v.as.str.len = total;
    if (need > sizeof(stack_buf)) free(out);
    return v;
}

RAD_API RadValue rad_contains(RadValue s, RadValue sub) {
    if (s.tag == RV_STR) {
        if (sub.tag != RV_STR) return rad_make_bool(false);
        if (!s.as.str.data || !sub.as.str.data) return rad_make_bool(false);
        if (sub.as.str.len == 0) return rad_make_bool(true);
        if (sub.as.str.len > s.as.str.len) return rad_make_bool(false);
        for (int64_t i = 0; i <= s.as.str.len - sub.as.str.len; i++) {
            if (memcmp(s.as.str.data + i, sub.as.str.data, sub.as.str.len) == 0) {
                return rad_make_bool(true);
            }
        }
        return rad_make_bool(false);
    }
    if (s.tag == RV_LIST_INT && s.as.list_i) {
        int64_t needle = rad_to_int(sub);
        for (int64_t i = 0; i < s.as.list_i->len; i++) {
            if (s.as.list_i->data[i] == needle) return rad_make_bool(true);
        }
        return rad_make_bool(false);
    }
    if (s.tag == RV_LIST && s.as.list) {
        for (int64_t i = 0; i < s.as.list->len; i++) {
            if (rad_is_truthy(rad_eq(s.as.list->data[i], sub))) return rad_make_bool(true);
        }
        return rad_make_bool(false);
    }
    if (s.tag == RV_MAP) {
        return rad_map_contains(s, sub);
    }
    return rad_make_bool(false);
}

RAD_API RadValue rad_replace(RadValue s, RadValue old_s, RadValue new_s) {
    if (s.tag != RV_STR || old_s.tag != RV_STR || new_s.tag != RV_STR) return s;
    if (!s.as.str.data || !old_s.as.str.data || old_s.as.str.len == 0) return s;
    const char *nd = new_s.as.str.data ? new_s.as.str.data : "";
    int64_t nlen = new_s.as.str.data ? new_s.as.str.len : 0;
    char *data = NULL;
    int64_t len = 0, cap = 0;
    int64_t i = 0;
    while (i <= s.as.str.len - old_s.as.str.len) {
        if (memcmp(s.as.str.data + i, old_s.as.str.data, old_s.as.str.len) == 0) {
            if (nlen > 0) {
                int64_t need = len + nlen;
                if (need > cap) { int64_t nc = cap == 0 ? 256 : cap; while (nc < need) nc *= 2; data = (char *)realloc(data, (size_t)nc); cap = nc; }
                memcpy(data + len, nd, (size_t)nlen);
                len += nlen;
            }
            i += old_s.as.str.len;
        } else {
            int64_t need = len + 1;
            if (need > cap) { int64_t nc = cap == 0 ? 256 : cap; while (nc < need) nc *= 2; data = (char *)realloc(data, (size_t)nc); cap = nc; }
            data[len++] = s.as.str.data[i];
            i++;
        }
    }
    while (i < s.as.str.len) {
        int64_t need = len + 1;
        if (need > cap) { int64_t nc = cap == 0 ? 256 : cap; while (nc < need) nc *= 2; data = (char *)realloc(data, (size_t)nc); cap = nc; }
        data[len++] = s.as.str.data[i];
        i++;
    }
    if (len == 0) { free(data); return rad_make_str(""); }
    RadValue v; v.tag = RV_STR;
    v.as.str.data = (char *)rad_intern_string(data, len);
    v.as.str.len = len;
    free(data);
    return v;
}

/* ========== Type casting ========== */

RAD_API RadValue rad_to_int_val(RadValue v) {
    switch (v.tag) {
    case RV_INT: return v;
    case RV_FLOAT: return rad_make_int((int64_t)v.as.f);
    case RV_BOOL: return rad_make_int(v.as.b ? 1 : 0);
    case RV_STR: {
        char *endptr = NULL;
        long long val = strtoll(v.as.str.data ? v.as.str.data : "", &endptr, 10);
        if (v.as.str.data && endptr != v.as.str.data && endptr && *endptr == '\0') {
            return rad_make_int((int64_t)val);
        }
        return rad_make_int(0);
    }
    default: return rad_make_int(0);
    }
}

RAD_API RadValue rad_to_float(RadValue v) {
    switch (v.tag) {
    case RV_FLOAT: return v;
    case RV_INT: return rad_make_float((double)v.as.i);
    case RV_BOOL: return rad_make_float(v.as.b ? 1.0 : 0.0);
    case RV_STR: {
        char *endptr = NULL;
        double val = strtod(v.as.str.data ? v.as.str.data : "", &endptr);
        if (v.as.str.data && endptr != v.as.str.data && endptr && *endptr == '\0') {
            return rad_make_float(val);
        }
        return rad_make_float(0.0);
    }
    default: return rad_make_float(0.0);
    }
}

RAD_API RadValue rad_int_div(RadValue a, RadValue b) {
    int64_t lhs = rad_to_int(a);
    int64_t rhs = rad_to_int(b);
    if (rhs == 0) {
        fprintf(stderr, "Runtime error: division by zero\n");
        exit(1);
    }
    return rad_make_int(lhs / rhs);
}

/* ========== I/O builtins ========== */

RAD_API RadValue rad_eprint_many(RadValue *vals, int64_t n) {
    if (!vals || n <= 0) {
        fputc('\n', stderr);
        return rad_make_nil();
    }
    for (int64_t i = 0; i < n; i++) {
        if (i > 0) fputc(' ', stderr);
        RadValue v = vals[i];
        if (v.tag == RV_STR && v.as.str.data) {
            fwrite(v.as.str.data, 1, (size_t)v.as.str.len, stderr);
        } else {
            RadValue s = str(v);
            if (s.tag == RV_STR && s.as.str.data) {
                fwrite(s.as.str.data, 1, (size_t)s.as.str.len, stderr);
            }
        }
    }
    fputc('\n', stderr);
    return rad_make_nil();
}

RAD_API RadValue rad_eprint(RadValue v) {
    return rad_eprint_many(&v, 1);
}

RAD_API RadValue rad_write_stdout(RadValue v) {
    if (v.tag == RV_STR && v.as.str.data) {
        fwrite(v.as.str.data, 1, (size_t)v.as.str.len, stdout);
    } else {
        RadValue s = str(v);
        if (s.tag == RV_STR && s.as.str.data) fwrite(s.as.str.data, 1, (size_t)s.as.str.len, stdout);
    }
    fflush(stdout);
    return rad_make_nil();
}

RAD_API RadValue rad_write_stderr(RadValue v) {
    if (v.tag == RV_STR && v.as.str.data) {
        fwrite(v.as.str.data, 1, (size_t)v.as.str.len, stderr);
    } else {
        RadValue s = str(v);
        if (s.tag == RV_STR && s.as.str.data) fwrite(s.as.str.data, 1, (size_t)s.as.str.len, stderr);
    }
    fflush(stderr);
    return rad_make_nil();
}

/* Debug helper: log to stderr and return the value unchanged (C backend).
 * When compiling with -DRAD_RELEASE, runtime.h defines rad_debug_trace as a no-op macro. */
#ifndef RAD_RELEASE
RAD_API RadValue rad_debug_trace(RadValue v) {
    fprintf(stderr, "DEBUG: ");
    if (v.tag == RV_STR && v.as.str.data) fprintf(stderr, "%s", v.as.str.data);
    else { RadValue s = str(v); if (s.as.str.data) fprintf(stderr, "%s", s.as.str.data); }
    fprintf(stderr, "\n");
    return v;
}
#endif

RAD_API RadValue rad_gc_collect(void) {
    static const int64_t sweep_seq[] = {3, 11, 7, 5, 0};
    int64_t idx = rad_gc_collect_calls;
    if (idx < 0) idx = 0;
    if (idx >= (int64_t)(sizeof(sweep_seq) / sizeof(sweep_seq[0]))) {
        idx = (int64_t)(sizeof(sweep_seq) / sizeof(sweep_seq[0])) - 1;
    }
    rad_gc_collect_calls++;
    return rad_make_int(sweep_seq[idx]);
}

RAD_API RadValue rad_flush(void) { fflush(stdout); return rad_make_nil(); }

/* ========== Higher-order functions ========== */

RAD_API RadValue rad_hof_map(RadValue lst, RadValue fn) {
    RadList *xs = rad_list_new();
    int64_t n = 0;
    if (lst.tag == RV_LIST && lst.as.list) n = lst.as.list->len;
    if (lst.tag == RV_LIST_INT && lst.as.list_i) n = lst.as.list_i->len;
    for (int64_t i = 0; i < n; i++) {
        RadValue elem = rad_index(lst, rad_make_int(i));
        RadValue result = rad_call(fn, &elem, 1);
        rad_list_push(xs, result);
    }
    RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
}

RAD_API RadValue rad_hof_filter(RadValue lst, RadValue fn) {
    RadList *xs = rad_list_new();
    int64_t n = 0;
    if (lst.tag == RV_LIST && lst.as.list) n = lst.as.list->len;
    if (lst.tag == RV_LIST_INT && lst.as.list_i) n = lst.as.list_i->len;
    for (int64_t i = 0; i < n; i++) {
        RadValue elem = rad_index(lst, rad_make_int(i));
        RadValue result = rad_call(fn, &elem, 1);
        if (rad_is_truthy(result)) rad_list_push(xs, elem);
    }
    RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
}

RAD_API RadValue rad_hof_reduce(RadValue lst, RadValue init, RadValue fn) {
    RadValue acc = init;
    int64_t n = 0;
    if (lst.tag == RV_LIST && lst.as.list) n = lst.as.list->len;
    if (lst.tag == RV_LIST_INT && lst.as.list_i) n = lst.as.list_i->len;
    for (int64_t i = 0; i < n; i++) {
        RadValue elem = rad_index(lst, rad_make_int(i));
        RadValue args[2] = { acc, elem };
        acc = rad_call(fn, args, 2);
    }
    return acc;
}

RAD_API RadValue rad_hof_sort_by(RadValue lst, RadValue fn) {
    int64_t n = 0;
    if (lst.tag == RV_LIST && lst.as.list) n = lst.as.list->len;
    if (lst.tag == RV_LIST_INT && lst.as.list_i) n = lst.as.list_i->len;
    if (n == 0) return rad_make_list();
    RadValue stack_arr[32];
    RadValue *arr;
    if (n <= (int64_t)(sizeof(stack_arr) / sizeof(stack_arr[0]))) {
        arr = stack_arr;
    } else {
        arr = (RadValue *)malloc((size_t)n * sizeof(RadValue));
        if (!arr) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
    }
    for (int64_t i = 0; i < n; i++) arr[i] = rad_index(lst, rad_make_int(i));
    for (int64_t i = 1; i < n; i++) {
        RadValue key = arr[i];
        RadValue key_val = rad_call(fn, &key, 1);
        int64_t j = i - 1;
        while (j >= 0) {
            RadValue j_val = rad_call(fn, &arr[j], 1);
            if (!rad_is_truthy(rad_gt(j_val, key_val))) break;
            arr[j + 1] = arr[j]; j--;
        }
        arr[j + 1] = key;
    }
    RadValue result = rad_list_literal(arr, n);
    if (n > (int64_t)(sizeof(stack_arr) / sizeof(stack_arr[0]))) free(arr);
    return result;
}

RAD_API RadValue rad_hof_flat_map(RadValue lst, RadValue fn) {
    RadList *xs = rad_list_new();
    int64_t n = 0;
    if (lst.tag == RV_LIST && lst.as.list) n = lst.as.list->len;
    if (lst.tag == RV_LIST_INT && lst.as.list_i) n = lst.as.list_i->len;
    for (int64_t i = 0; i < n; i++) {
        RadValue elem = rad_index(lst, rad_make_int(i));
        RadValue sub = rad_call(fn, &elem, 1);
        int64_t sn = 0;
        if (sub.tag == RV_LIST && sub.as.list) sn = sub.as.list->len;
        if (sub.tag == RV_LIST_INT && sub.as.list_i) sn = sub.as.list_i->len;
        for (int64_t j = 0; j < sn; j++)
            rad_list_push(xs, rad_index(sub, rad_make_int(j)));
    }
    RadValue v; v.tag = RV_LIST; v.as.list = xs; return v;
}

RAD_API RadValue rad_hof_find(RadValue lst, RadValue fn) {
    int64_t n = 0;
    if (lst.tag == RV_LIST && lst.as.list) n = lst.as.list->len;
    if (lst.tag == RV_LIST_INT && lst.as.list_i) n = lst.as.list_i->len;
    for (int64_t i = 0; i < n; i++) {
        RadValue elem = rad_index(lst, rad_make_int(i));
        RadValue result = rad_call(fn, &elem, 1);
        if (rad_is_truthy(result)) return rad_make_some(elem);
    }
    return rad_make_none();
}

static int rad_compare_keys(RadValue a, RadValue b, const char *fn_name) {
    if (a.tag == RV_INT && b.tag == RV_INT) return (a.as.i > b.as.i) - (a.as.i < b.as.i);
    double da = (a.tag == RV_FLOAT) ? a.as.f : (double)a.as.i;
    double db = (b.tag == RV_FLOAT) ? b.as.f : (double)b.as.i;
    if ((a.tag == RV_INT || a.tag == RV_FLOAT) && (b.tag == RV_INT || b.tag == RV_FLOAT))
        return (da > db) - (da < db);
    if (a.tag == RV_STR && b.tag == RV_STR) {
        int64_t mn = a.as.str.len < b.as.str.len ? a.as.str.len : b.as.str.len;
        int c = memcmp(a.as.str.data, b.as.str.data, (size_t)mn);
        if (c != 0) return c;
        return (a.as.str.len > b.as.str.len) - (a.as.str.len < b.as.str.len);
    }
    if (a.tag == RV_BOOL && b.tag == RV_BOOL)
        return (int)a.as.b - (int)b.as.b;
    RadValue ta = rad_typeof(a);
    RadValue tb = rad_typeof(b);
    fprintf(stderr, "Runtime error: %s() key function returned incomparable types: %s and %s\n",
            fn_name,
            (ta.tag == RV_STR && ta.as.str.data) ? ta.as.str.data : "unknown",
            (tb.tag == RV_STR && tb.as.str.data) ? tb.as.str.data : "unknown");
    exit(1);
}

RAD_API RadValue rad_hof_max_by(RadValue lst, RadValue fn) {
    int64_t n = 0;
    if (lst.tag == RV_LIST && lst.as.list) n = lst.as.list->len;
    if (lst.tag == RV_LIST_INT && lst.as.list_i) n = lst.as.list_i->len;
    if (n == 0) return rad_make_none();
    RadValue best = rad_index(lst, rad_make_int(0));
    RadValue best_key = rad_call(fn, &best, 1);
    for (int64_t i = 1; i < n; i++) {
        RadValue elem = rad_index(lst, rad_make_int(i));
        RadValue key = rad_call(fn, &elem, 1);
        if (rad_compare_keys(key, best_key, "max_by") > 0) { best = elem; best_key = key; }
    }
    return rad_make_some(best);
}

RAD_API RadValue rad_hof_min_by(RadValue lst, RadValue fn) {
    int64_t n = 0;
    if (lst.tag == RV_LIST && lst.as.list) n = lst.as.list->len;
    if (lst.tag == RV_LIST_INT && lst.as.list_i) n = lst.as.list_i->len;
    if (n == 0) return rad_make_none();
    RadValue best = rad_index(lst, rad_make_int(0));
    RadValue best_key = rad_call(fn, &best, 1);
    for (int64_t i = 1; i < n; i++) {
        RadValue elem = rad_index(lst, rad_make_int(i));
        RadValue key = rad_call(fn, &elem, 1);
        if (rad_compare_keys(key, best_key, "min_by") < 0) { best = elem; best_key = key; }
    }
    return rad_make_some(best);
}

RAD_API RadValue rad_format(RadValue fmt, RadValue *vals, int64_t nvals) {
    RadValue b = rad_buffer_new();
    if (fmt.tag != RV_STR || !fmt.as.str.data) {
        return str(fmt);
    }
    int64_t ai = 0;
    for (int64_t i = 0; i < fmt.as.str.len; i++) {
        char c = fmt.as.str.data[i];
        if (c == '{' && i + 1 < fmt.as.str.len && fmt.as.str.data[i + 1] == '}') {
            if (ai < nvals) {
                rad_buffer_append(b, str(vals[ai]));
                ai++;
            } else {
                rad_buffer_append(b, rad_make_str("{}"));
            }
            i++;
        } else {
            char tmp[2] = { c, 0 };
            rad_buffer_append(b, rad_make_str(tmp));
        }
    }
    return rad_buffer_to_str(b);
}

RAD_API RadValue rad_entries(RadValue m) {
    RadValue out = rad_make_list();
    if (m.tag != RV_MAP || !m.as.map) return out;
    for (int64_t i = 0; i < m.as.map->len; i++) {
        RadValue _pair_elems[2] = { m.as.map->keys[i], m.as.map->vals[i] };
        RadValue pair = rad_make_tuple(_pair_elems, 2);
        out = rad_push(out, pair);
    }
    return out;
}

RAD_API RadValue rad_merge(RadValue a, RadValue b) {
    RadValue out = rad_make_map();
    if (a.tag == RV_MAP && a.as.map) {
        for (int64_t i = 0; i < a.as.map->len; i++) out = rad_map_set(out, a.as.map->keys[i], a.as.map->vals[i]);
    }
    if (b.tag == RV_MAP && b.as.map) {
        for (int64_t i = 0; i < b.as.map->len; i++) out = rad_map_set(out, b.as.map->keys[i], b.as.map->vals[i]);
    }
    return out;
}

RAD_API RadValue rad_group_by(RadValue lst, RadValue fn) {
    RadValue out = rad_make_map();
    int64_t n = 0;
    if (lst.tag == RV_LIST && lst.as.list) n = lst.as.list->len;
    if (lst.tag == RV_LIST_INT && lst.as.list_i) n = lst.as.list_i->len;
    for (int64_t i = 0; i < n; i++) {
        RadValue elem = rad_index(lst, rad_make_int(i));
        RadValue key = rad_call(fn, &elem, 1);
        RadValue bucket = rad_map_get(out, key);
        if (bucket.tag != RV_LIST || !bucket.as.list) {
            bucket = rad_make_list();
            out = rad_map_set(out, key, bucket);
        }
        bucket = rad_push(bucket, elem);
        out = rad_map_set(out, key, bucket);
    }
    return out;
}

RAD_API void rad_register_transition(int64_t from_comp, const char *event_name, int64_t to_comp, int guard_true) {
    if (!event_name) return;
    if (rad_transition_rules_len >= rad_transition_rules_cap) {
        int64_t new_cap = rad_transition_rules_cap == 0 ? 16 : rad_transition_rules_cap * 2;
        rad_transition_rules = (RadTransitionRule *)rad_arena_realloc(
            rad_transition_rules,
            (size_t)rad_transition_rules_cap * sizeof(RadTransitionRule),
            (size_t)new_cap * sizeof(RadTransitionRule));
        rad_transition_rules_cap = new_cap;
    }
    rad_transition_rules[rad_transition_rules_len].from_comp = from_comp;
    rad_transition_rules[rad_transition_rules_len].event_name = event_name;
    rad_transition_rules[rad_transition_rules_len].to_comp = to_comp;
    rad_transition_rules[rad_transition_rules_len].guard_true = guard_true ? 1 : 0;
    rad_transition_rules_len++;
}

RAD_API RadValue rad_transition(RadValue state, RadValue event_name) {
    if (state.tag != RV_ENTITY || event_name.tag != RV_STR || !event_name.as.str.data) {
        return rad_make_nil();
    }
    bool has_source_state = false;
    for (int64_t i = 0; i < rad_transition_rules_len; i++) {
        RadTransitionRule *r = &rad_transition_rules[i];
        if (!rad_is_truthy(rad_ecs_has(state, r->from_comp))) continue;
        has_source_state = true;
        if (strcmp(event_name.as.str.data, r->event_name) == 0) {
            if (!r->guard_true) {
                continue;
            }
            RadValue next = rad_spawn();
            rad_ecs_set(next, r->to_comp, rad_make_bool(true));

            return rad_make_result_ok(next);
        }
    }

    if (has_source_state) {
        RadValue variant = rad_variant_of(state);
        const char *state_name = (variant.tag == RV_STR && variant.as.str.data) ? variant.as.str.data : "unknown";
        int msg_need = snprintf(NULL, 0, "No transition on '%s' from state '%s'", event_name.as.str.data, state_name);
        char stack_buf[256];
        char *msg;
        size_t msg_sz = (size_t)msg_need + 1;
        if (msg_sz <= sizeof(stack_buf)) {
            msg = stack_buf;
        } else {
            msg = (char *)malloc(msg_sz);
            if (!msg) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
        }
        snprintf(msg, msg_sz, "No transition on '%s' from state '%s'", event_name.as.str.data, state_name);
        RadValue result = rad_make_result_err(rad_make_str(msg));
        if (msg_sz > sizeof(stack_buf)) free(msg);
        return result;
    }

    return rad_make_nil();
}

/* ========== RNG (xorshift64 — matches VM) ========== */

static uint64_t g_rng_state = 12345;

RAD_API RadValue rad_rand_seed(RadValue v) {
    uint64_t s = (uint64_t)rad_to_int(v);
    g_rng_state = (s == 0) ? 0xD1B54A32D192ED03ULL : s;
    return rad_make_nil();
}

static uint64_t rng_next(void) {
    if (g_rng_state == 0) g_rng_state = 0xD1B54A32D192ED03ULL;
    uint64_t x = g_rng_state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    g_rng_state = x;
    return x * 0x2545F4914F6CDD1DULL;
}

static uint64_t rng_bounded(uint64_t bound) {
    if (bound <= 1) return 0;
    uint64_t threshold = UINT64_MAX - (UINT64_MAX % bound);
    for (;;) {
        uint64_t n = rng_next();
        if (n < threshold) return n % bound;
    }
}

RAD_API RadValue rad_rand_int(RadValue lo, RadValue hi) {
    int64_t a = rad_to_int(lo), b = rad_to_int(hi);
    if (b < a) return rad_make_int(a);
    if (a == INT64_MIN && b == INT64_MAX) return rad_make_int((int64_t)rng_next());
    uint64_t width = (uint64_t)b - (uint64_t)a + 1;
    return rad_make_int(a + (int64_t)rng_bounded(width));
}

RAD_API RadValue rad_rand_float(void) {
    return rad_make_float((double)(rng_next() >> 11) / (double)(1ULL << 53));
}

RAD_API RadValue rad_rand_bool(void) { return rad_make_bool((rng_next() & 1) == 1); }

/* ========== Index assignment ========== */

RAD_API RadValue rad_index_set(RadValue obj, RadValue idx, RadValue val) {
    if (obj.tag == RV_MAP && obj.as.map) {
        return rad_map_set(obj, idx, val);
    }
    if (obj.tag == RV_LIST && obj.as.list) {
        int64_t i = rad_to_int(idx);
        if (i >= 0 && i < obj.as.list->len) obj.as.list->data[i] = val;
    }
    if (obj.tag == RV_LIST_INT && obj.as.list_i) {
        int64_t i = rad_to_int(idx);
        if (i >= 0 && i < obj.as.list_i->len) obj.as.list_i->data[i] = rad_to_int(val);
    }
    return obj;
}

RAD_API RadValue rad_abs(RadValue v) {
    if (v.tag == RV_INT) return rad_make_int(v.as.i < 0 ? -v.as.i : v.as.i);
    if (v.tag == RV_FLOAT) return rad_make_float(v.as.f < 0 ? -v.as.f : v.as.f);
    return v;
}

/* ========== Tuples ========== */

RAD_API RadValue rad_make_tuple(RadValue *elems, int64_t n) {
    RadList *t = (RadList *)rad_arena_alloc(sizeof(RadList));
    t->len = n; t->cap = n;
    t->data = (RadValue *)rad_arena_alloc(sizeof(RadValue) * n);
    for (int64_t i = 0; i < n; i++) t->data[i] = elems[i];
    RadValue v; v.tag = RV_TUPLE; v.as.tuple = t; return v;
}

RAD_API RadValue rad_tuple_get(RadValue tup, int64_t idx) {
    if (tup.tag != RV_TUPLE || !tup.as.tuple || idx < 0 || idx >= tup.as.tuple->len)
        return rad_make_nil();
    return tup.as.tuple->data[idx];
}

RAD_API int64_t rad_tuple_len(RadValue tup) {
    if (tup.tag == RV_TUPLE && tup.as.tuple) return tup.as.tuple->len;
    return 0;
}

static inline bool rad_value_is_trivially_copyable(RadValue v) {
    switch (v.tag) {
    case RV_NIL: case RV_INT: case RV_FLOAT: case RV_BOOL:
    case RV_STR: case RV_FN: case RV_OPTION_NONE:
        return true;
    default:
        return false;
    }
}

/* ========== Struct component values (RV_STRUCT) ========== */

static void rad_fieldstore_cow(RadStruct *st) {
    if (!st || !st->store || st->store->refcount <= 1) return;
    RadFieldStore *old = st->store;
    old->refcount--;
    RadFieldStore *fs = (RadFieldStore *)rad_arena_alloc(sizeof(RadFieldStore));
    fs->len = old->len;
    fs->refcount = 1;
    if (old->len > 0 && old->fields) {
        fs->fields = (RadValue *)rad_arena_alloc((size_t)old->len * sizeof(RadValue));
        memcpy(fs->fields, old->fields, (size_t)old->len * sizeof(RadValue));
    } else {
        fs->fields = NULL;
    }
    st->store = fs;
}

RAD_API RadValue rad_make_struct_literal(RadValue *fields, int64_t n, int64_t layout_comp) {
    RadFieldStore *fs = (RadFieldStore *)rad_arena_alloc(sizeof(RadFieldStore));
    fs->len = n;
    fs->refcount = 1;
    if (n <= 0 || !fields) {
        fs->fields = NULL;
    } else {
        fs->fields = (RadValue *)rad_arena_alloc((size_t)n * sizeof(RadValue));
        for (int64_t i = 0; i < n; i++) {
            fs->fields[i] = rad_value_deep_copy(fields[i]);
        }
    }
    RadStruct *st = (RadStruct *)rad_arena_alloc(sizeof(RadStruct));
    st->layout_comp = layout_comp;
    st->store = fs;
    RadValue v;
    v.tag = RV_STRUCT;
    v.as.rst = st;
    return v;
}

RAD_API RadValue rad_value_get_comp_field(RadValue v, int64_t idx, int64_t field_comp_id) {
    if (v.tag == RV_STRUCT && v.as.rst && v.as.rst->store) {
        int64_t ord = -1;
        if (v.as.rst->layout_comp >= 0 && field_comp_id >= 0)
            ord = rad_struct_resolve_slot(v.as.rst->layout_comp, field_comp_id);
        if (ord < 0 && idx >= 0 && idx < v.as.rst->store->len)
            ord = idx;
        if (ord < 0 || ord >= v.as.rst->store->len) return rad_make_nil();
        RadValue field = v.as.rst->store->fields[ord];
        return rad_value_is_trivially_copyable(field) ? field : rad_value_deep_copy(field);
    }
    if (v.tag == RV_OPTION_SOME || v.tag == RV_RESULT_OK || v.tag == RV_RESULT_ERR) {
        if (!v.as.inner) return rad_make_nil();
        RadValue inner = *v.as.inner;
        return rad_value_is_trivially_copyable(inner) ? inner : rad_value_deep_copy(inner);
    }
    if (v.tag == RV_OPTION_NONE) return rad_make_nil();
    if (v.tag == RV_ENTITY)
        return rad_ecs_require(v, field_comp_id);
    fprintf(stderr, "rad runtime: field access expects struct or entity, got tag %d\n", (int)v.tag);
    exit(1);
}

RAD_API RadValue rad_value_comp_field_set(RadValue obj, int64_t idx, int64_t field_comp_id, RadValue val) {
    if (obj.tag == RV_STRUCT && obj.as.rst && obj.as.rst->store) {
        rad_fieldstore_cow(obj.as.rst);
        int64_t ord = -1;
        if (obj.as.rst->layout_comp >= 0 && field_comp_id >= 0)
            ord = rad_struct_resolve_slot(obj.as.rst->layout_comp, field_comp_id);
        if (ord < 0 && idx >= 0 && idx < obj.as.rst->store->len)
            ord = idx;
        if (ord >= 0 && ord < obj.as.rst->store->len)
            obj.as.rst->store->fields[ord] = rad_value_is_trivially_copyable(val) ? val : rad_value_deep_copy(val);
        return obj;
    }
    if (obj.tag == RV_ENTITY) {
        rad_ecs_set(obj, field_comp_id, val);
        return obj;
    }
    return obj;
}

RAD_API void rad_value_mut_set_field(RadValue obj, int64_t idx, int64_t field_comp_id, RadValue val) {
    if (obj.tag == RV_STRUCT && obj.as.rst && obj.as.rst->store) {
        rad_fieldstore_cow(obj.as.rst);
        int64_t ord = -1;
        if (obj.as.rst->layout_comp >= 0 && field_comp_id >= 0)
            ord = rad_struct_resolve_slot(obj.as.rst->layout_comp, field_comp_id);
        if (ord < 0 && idx >= 0 && idx < obj.as.rst->store->len)
            ord = idx;
        if (ord >= 0 && ord < obj.as.rst->store->len)
            obj.as.rst->store->fields[ord] = rad_value_is_trivially_copyable(val) ? val : rad_value_deep_copy(val);
        return;
    }
    if (obj.tag == RV_ENTITY)
        rad_ecs_set(obj, field_comp_id, val);
}

/* ========== Maps (FNV-1a hash + open addressing) ========== */

typedef struct {
    RadValue *keys;
    RadValue *vals;
    int64_t len;
    int64_t cap;
    int64_t *buckets;
    int64_t bucket_cap;
} RadMapStore;

static uint64_t rad_hash_value(RadValue v) {
    uint64_t h = 14695981039346656037ULL;
    h ^= (uint64_t)v.tag;
    h *= 1099511628211ULL;
    switch (v.tag) {
    case RV_INT:
    case RV_ENTITY:
        h ^= (uint64_t)v.as.i;
        h *= 1099511628211ULL;
        break;
    case RV_FLOAT: {
        uint64_t bits;
        memcpy(&bits, &v.as.f, sizeof(bits));
        h ^= bits;
        h *= 1099511628211ULL;
        break;
    }
    case RV_STR:
        if (v.as.str.data) {
            for (int64_t si = 0; si < v.as.str.len; si++) {
                h ^= (uint8_t)v.as.str.data[si];
                h *= 1099511628211ULL;
            }
        }
        break;
    case RV_BOOL:
        h ^= v.as.b ? 1ULL : 0ULL;
        h *= 1099511628211ULL;
        break;
    case RV_STRUCT:
        if (v.as.rst && v.as.rst->store) {
            h ^= (uint64_t)v.as.rst->layout_comp;
            h *= 1099511628211ULL;
            for (int64_t si = 0; si < v.as.rst->store->len; si++) {
                uint64_t fh = rad_hash_value(v.as.rst->store->fields[si]);
                h ^= fh;
                h *= 1099511628211ULL;
            }
        }
        break;
    default:
        h ^= (uint64_t)v.as.i;
        h *= 1099511628211ULL;
        break;
    }
    return h;
}

static void map_init_buckets(RadMapStore *m, int64_t bcap) {
    m->bucket_cap = bcap;
    m->buckets = (int64_t *)rad_arena_alloc(sizeof(int64_t) * bcap);
    memset(m->buckets, 0xFF, sizeof(int64_t) * bcap);
}

RAD_API RadValue rad_make_map(void) {
    RadMapStore *m = (RadMapStore *)rad_arena_alloc(sizeof(RadMapStore));
    m->len = 0; m->cap = 8;
    m->keys = (RadValue *)rad_arena_alloc(sizeof(RadValue) * 8);
    m->vals = (RadValue *)rad_arena_alloc(sizeof(RadValue) * 8);
    map_init_buckets(m, 16);
    RadValue v; v.tag = RV_MAP; v.as.map = (void *)m; return v;
}

static int64_t map_find(RadMapStore *m, RadValue key) {
    uint64_t h = rad_hash_value(key);
    int64_t mask = m->bucket_cap - 1;
    int64_t pos = (int64_t)(h & (uint64_t)mask);
    for (;;) {
        int64_t idx = m->buckets[pos];
        if (idx == -1) return -1;
        if (idx >= 0 && rad_is_truthy(rad_eq(m->keys[idx], key))) return idx;
        pos = (pos + 1) & mask;
    }
}

static void map_rehash(RadMapStore *m) {
    int64_t new_bcap = m->bucket_cap * 2;
    int64_t *old_buckets = m->buckets;
    (void)old_buckets;
    map_init_buckets(m, new_bcap);
    int64_t mask = new_bcap - 1;
    for (int64_t i = 0; i < m->len; i++) {
        uint64_t h = rad_hash_value(m->keys[i]);
        int64_t pos = (int64_t)(h & (uint64_t)mask);
        while (m->buckets[pos] >= 0) pos = (pos + 1) & mask;
        m->buckets[pos] = i;
    }
}

static void map_grow(RadMapStore *m) {
    int64_t nc = m->cap * 2;
    RadValue *nk = (RadValue *)rad_arena_alloc(sizeof(RadValue) * nc);
    RadValue *nv = (RadValue *)rad_arena_alloc(sizeof(RadValue) * nc);
    memcpy(nk, m->keys, sizeof(RadValue) * m->len);
    memcpy(nv, m->vals, sizeof(RadValue) * m->len);
    m->keys = nk; m->vals = nv; m->cap = nc;
}

static void map_bucket_insert(RadMapStore *m, int64_t entry_idx) {
    int64_t mask = m->bucket_cap - 1;
    uint64_t h = rad_hash_value(m->keys[entry_idx]);
    int64_t pos = (int64_t)(h & (uint64_t)mask);
    while (m->buckets[pos] >= 0) pos = (pos + 1) & mask;
    m->buckets[pos] = entry_idx;
}

RAD_API RadValue rad_map_set(RadValue mv, RadValue key, RadValue val) {
    if (mv.tag != RV_MAP || !mv.as.map) return mv;
    RadMapStore *m = (RadMapStore *)mv.as.map;
    int64_t idx = map_find(m, key);
    if (idx >= 0) { m->vals[idx] = val; return mv; }
    if (m->len >= m->cap) map_grow(m);
    if (m->len * 4 >= m->bucket_cap * 3) map_rehash(m);
    m->keys[m->len] = key;
    m->vals[m->len] = val;
    map_bucket_insert(m, m->len);
    m->len++;
    return mv;
}

RAD_API RadValue rad_map_get(RadValue mv, RadValue key) {
    if (mv.tag != RV_MAP || !mv.as.map) return rad_make_nil();
    RadMapStore *m = (RadMapStore *)mv.as.map;
    int64_t idx = map_find(m, key);
    return idx >= 0 ? m->vals[idx] : rad_make_nil();
}

RAD_API RadValue rad_map_keys(RadValue mv) {
    RadValue lst = rad_make_list();
    if (mv.tag != RV_MAP || !mv.as.map) return lst;
    RadMapStore *m = (RadMapStore *)mv.as.map;
    for (int64_t i = 0; i < m->len; i++) rad_list_push(lst.as.list, m->keys[i]);
    return lst;
}

RAD_API RadValue rad_keys(RadValue obj) {
    if (obj.tag == RV_MAP) {
        return rad_map_keys(obj);
    }
    if (obj.tag == RV_STRUCT && obj.as.rst && obj.as.rst->store) {
        RadValue lst = rad_make_list();
        int64_t lc = obj.as.rst->layout_comp;
        for (int64_t slot = 0; slot < obj.as.rst->store->len; slot++) {
            const char *found = NULL;
            for (int64_t r = 0; r < rad_layout_rule_count; r++) {
                if (rad_layout_rule_comp[r] == lc && rad_layout_rule_slot[r] == slot) {
                    found = rad_field_names[rad_layout_rule_field[r]];
                    break;
                }
            }
            if (found) rad_list_push(lst.as.list, rad_make_str(found));
        }
        return lst;
    }
    RadValue lst = rad_make_list();
    if (obj.tag != RV_ENTITY) return lst;
    int64_t eid = obj.as.entity_id;
    if (eid < 0 || eid >= rad_mask_cap) return lst;
    for (int64_t comp_id = 0; comp_id < rad_next_component_id && comp_id < RAD_MAX_COMPONENTS; comp_id++) {
        const char *fname = rad_field_names[comp_id];
        if (!fname) continue;
        if (rad_mask_has(eid, comp_id)) {
            rad_list_push(lst.as.list, rad_make_str(fname));
        }
    }
    return lst;
}

RAD_API RadValue rad_map_values(RadValue mv) {
    RadValue lst = rad_make_list();
    if (mv.tag != RV_MAP || !mv.as.map) return lst;
    RadMapStore *m = (RadMapStore *)mv.as.map;
    for (int64_t i = 0; i < m->len; i++) rad_list_push(lst.as.list, m->vals[i]);
    return lst;
}

RAD_API int64_t rad_map_len(RadValue mv) {
    if (mv.tag == RV_MAP && mv.as.map) return ((RadMapStore *)mv.as.map)->len;
    return 0;
}

RAD_API RadValue rad_map_contains(RadValue mv, RadValue key) {
    if (mv.tag != RV_MAP || !mv.as.map) return rad_make_bool(false);
    return rad_make_bool(map_find((RadMapStore *)mv.as.map, key) >= 0);
}

RAD_API RadValue rad_map_remove(RadValue mv, RadValue key) {
    if (mv.tag != RV_MAP || !mv.as.map) return mv;
    RadMapStore *m = (RadMapStore *)mv.as.map;
    int64_t idx = map_find(m, key);
    if (idx < 0) return mv;
    int64_t last = m->len - 1;
    if (idx != last) {
        m->keys[idx] = m->keys[last];
        m->vals[idx] = m->vals[last];
    }
    m->len--;
    map_rehash(m);
    return mv;
}

RAD_API RadValue rad_map_literal(RadValue *keys, RadValue *vals, int64_t n) {
    RadValue mv = rad_make_map();
    RadMapStore *m = (RadMapStore *)mv.as.map;
    if (n > m->cap) {
        m->cap = n;
        m->keys = (RadValue *)rad_arena_alloc(sizeof(RadValue) * n);
        m->vals = (RadValue *)rad_arena_alloc(sizeof(RadValue) * n);
    }
    int64_t bcap = 16;
    while (bcap < n * 2) bcap *= 2;
    if (bcap > m->bucket_cap) map_init_buckets(m, bcap);
    for (int64_t i = 0; i < n; i++) {
        m->keys[i] = keys[i];
        m->vals[i] = vals[i];
        map_bucket_insert(m, i);
    }
    m->len = n;
    return mv;
}

typedef struct {
    const RadWorldFork *fork;
    bool use_fork;
} RadValueCopySource;

static int64_t rad_copy_source_num_components(const RadValueCopySource *src) {
    if (!src) return 0;
    if (src->use_fork) {
        if (!src->fork) return 0;
        return src->fork->num_components;
    }
    return rad_next_component_id;
}

static bool rad_copy_source_entity_alive(const RadValueCopySource *src, int64_t eid) {
    if (!src || eid < 0) return false;
    if (!src->use_fork) {
        return rad_is_alive(eid);
    }
    if (!src->fork || !src->fork->entity_alive_ref || !src->fork->entity_alive_ref->data) return false;
    if (eid >= src->fork->mask_cap) return false;
    int64_t word = eid >> 6;
    if (word < 0 || word >= src->fork->entity_alive_ref->len_words) return false;
    return (src->fork->entity_alive_ref->data[word] & (1ULL << (eid & 63))) != 0;
}

static bool rad_copy_source_entity_has_component(const RadValueCopySource *src, int64_t eid, int64_t comp_id) {
    if (!src || eid < 0 || comp_id < 0) return false;
    int64_t max_comp = rad_copy_source_num_components(src);
    if (comp_id >= max_comp) return false;
    if (!src->use_fork) {
        return rad_mask_has(eid, comp_id);
    }
    if (!src->fork || eid >= src->fork->mask_cap) return false;
    if (!src->fork->entity_masks_ref || !src->fork->entity_masks_ref->data) return false;
    if (src->fork->mask_words <= 0) return false;
    int64_t word = comp_id >> 6;
    if (word >= src->fork->mask_words) return false;
    int64_t idx = eid * src->fork->mask_words + word;
    return (src->fork->entity_masks_ref->data[idx] & (1ULL << (comp_id & 63))) != 0;
}

static RadValue rad_copy_source_entity_component(const RadValueCopySource *src, int64_t eid, int64_t comp_id, bool *ok) {
    if (ok) *ok = false;
    if (!rad_copy_source_entity_has_component(src, eid, comp_id)) return rad_make_nil();

    if (!src->use_fork) {
        RadComponentStore *store = &rad_components[comp_id];
        if (!store->column || eid >= store->column->capacity) return rad_make_nil();
        if (ok) *ok = true;
        return store->column->data[eid];
    }

    if (!src->fork || !src->fork->columns) return rad_make_nil();
    RadRefColumn *col = src->fork->columns[comp_id];
    if (!col || !col->data || eid >= col->capacity) return rad_make_nil();
    if (ok) *ok = true;
    return col->data[eid];
}

static bool rad_copy_source_entity_is_value_like(const RadValueCopySource *src, int64_t eid) {
    if (!rad_copy_source_entity_alive(src, eid)) return false;

    int64_t max_comp = rad_copy_source_num_components(src);

    int64_t scan_max = max_comp;
    if (scan_max > RAD_MAX_COMPONENTS) scan_max = RAD_MAX_COMPONENTS;
    for (int64_t comp_id = 0; comp_id < scan_max; comp_id++) {
        if (rad_field_names[comp_id] && rad_copy_source_entity_has_component(src, eid, comp_id)) return true;
        if (rad_state_variant_ids[comp_id] && rad_copy_source_entity_has_component(src, eid, comp_id)) return true;
    }
    return false;
}

static RadValue rad_value_deep_copy_with_source(RadValue v, const RadValueCopySource *src) {
    switch (v.tag) {
    case RV_NIL:
    case RV_INT:
    case RV_FLOAT:
    case RV_BOOL:
    case RV_STR:
    case RV_FN:
    case RV_WORLD_FORK:
        return v;
    case RV_OPTION_NONE:
        return v;
    case RV_OPTION_SOME:
        return rad_make_some(rad_value_deep_copy_with_source(*v.as.inner, src));
    case RV_RESULT_OK:
        return rad_make_result_ok(rad_value_deep_copy_with_source(*v.as.inner, src));
    case RV_RESULT_ERR:
        return rad_make_result_err(rad_value_deep_copy_with_source(*v.as.inner, src));
    case RV_ENTITY: {
        int64_t eid = v.as.entity_id;
        if (!rad_copy_source_entity_is_value_like(src, eid)) {
            return v;
        }
        RadValue out = rad_spawn();
        int64_t max_comp = rad_copy_source_num_components(src);
        for (int64_t comp_id = 0; comp_id < max_comp; comp_id++) {
            if (!rad_copy_source_entity_has_component(src, eid, comp_id)) continue;
            bool ok = false;
            RadValue field_val = rad_copy_source_entity_component(src, eid, comp_id, &ok);
            if (!ok) continue;
            rad_ecs_set(out, comp_id, rad_value_deep_copy_with_source(field_val, src));
        }
        return out;
    }
    case RV_LIST_INT: {
        if (!v.as.list_i) return v;
        RadIntList *src_list = v.as.list_i;
        RadIntList *dst = rad_list_int_new();
        if (src_list->len > 0) {
            rad_list_int_reserve(dst, src_list->len);
            memcpy(dst->data, src_list->data, (size_t)src_list->len * sizeof(int64_t));
            dst->len = src_list->len;
        }
        RadValue out = v;
        out.as.list_i = dst;
        return out;
    }
    case RV_LIST: {
        if (!v.as.list) return v;
        RadList *src_list = v.as.list;
        RadList *dst = rad_list_new();
        if (src_list->len > 0) {
            rad_list_reserve(dst, src_list->len);
            for (int64_t i = 0; i < src_list->len; i++) {
                dst->data[i] = rad_value_deep_copy_with_source(src_list->data[i], src);
            }
            dst->len = src_list->len;
        }
        RadValue out = v;
        out.as.list = dst;
        return out;
    }
    case RV_TUPLE: {
        if (!v.as.tuple) return v;
        int64_t n = v.as.tuple->len;
        RadValue stack_elems[16];
        RadValue *elems;
        if (n <= (int64_t)(sizeof(stack_elems) / sizeof(stack_elems[0]))) {
            elems = n > 0 ? stack_elems : NULL;
        } else {
            elems = (RadValue *)malloc((size_t)n * sizeof(RadValue));
            if (!elems) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
        }
        for (int64_t i = 0; i < n; i++) {
            elems[i] = rad_value_deep_copy_with_source(v.as.tuple->data[i], src);
        }
        RadValue result = rad_make_tuple(elems, n);
        if (n > (int64_t)(sizeof(stack_elems) / sizeof(stack_elems[0]))) free(elems);
        return result;
    }
    case RV_MAP: {
        if (!v.as.map) return v;
        RadMapStore *src_map = (RadMapStore *)v.as.map;
        RadValue out = rad_make_map();
        RadMapStore *dst = (RadMapStore *)out.as.map;
        if (src_map->cap > dst->cap) {
            dst->cap = src_map->cap;
            dst->keys = (RadValue *)rad_arena_alloc((size_t)src_map->cap * sizeof(RadValue));
            dst->vals = (RadValue *)rad_arena_alloc((size_t)src_map->cap * sizeof(RadValue));
        }
        if (src_map->bucket_cap != dst->bucket_cap) {
            map_init_buckets(dst, src_map->bucket_cap);
        } else {
            memset(dst->buckets, 0xFF, sizeof(int64_t) * (size_t)dst->bucket_cap);
        }
        for (int64_t i = 0; i < src_map->len; i++) {
            dst->keys[i] = rad_value_deep_copy_with_source(src_map->keys[i], src);
            dst->vals[i] = rad_value_deep_copy_with_source(src_map->vals[i], src);
        }
        dst->len = src_map->len;
        for (int64_t i = 0; i < dst->len; i++) {
            map_bucket_insert(dst, i);
        }
        return out;
    }
    case RV_BITSET: {
        if (!v.as.bitset) return v;
        RadBitSet *src_bs = v.as.bitset;
        RadBitSet *dst = rad_bitset_new_impl();
        if (src_bs->capacity > 0) {
            dst->words = (uint64_t *)rad_arena_alloc((size_t)src_bs->capacity * sizeof(uint64_t));
            memcpy(dst->words, src_bs->words, (size_t)src_bs->capacity * sizeof(uint64_t));
            dst->capacity = src_bs->capacity;
        }
        RadValue out = v;
        out.as.bitset = dst;
        return out;
    }
    case RV_BUFFER: {
        if (!v.as.buffer) return v;
        RadBuffer *src_buf = v.as.buffer;
        RadBuffer *dst = (RadBuffer *)rad_arena_alloc(sizeof(RadBuffer));
        dst->len = src_buf->len;
        dst->cap = src_buf->cap;
        if (src_buf->cap > 0) {
            dst->data = (char *)rad_arena_alloc((size_t)src_buf->cap);
            if (src_buf->len > 0 && src_buf->data) {
                memcpy(dst->data, src_buf->data, (size_t)src_buf->len);
            }
        } else {
            dst->data = NULL;
        }
        RadValue out = v;
        out.as.buffer = dst;
        return out;
    }
    case RV_STRUCT: {
        if (!v.as.rst || !v.as.rst->store) return v;
        v.as.rst->store->refcount++;
        RadStruct *st = (RadStruct *)rad_arena_alloc(sizeof(RadStruct));
        st->layout_comp = v.as.rst->layout_comp;
        st->store = v.as.rst->store;
        RadValue out;
        out.tag = RV_STRUCT;
        out.as.rst = st;
        return out;
    }
    default:
        return v;
    }
}

RAD_API RadValue rad_value_deep_copy(RadValue v) {
    RadValueCopySource src;
    src.fork = NULL;
    src.use_fork = false;
    return rad_value_deep_copy_with_source(v, &src);
}

static RadValue rad_value_deep_copy_from_fork(RadValue v, const RadWorldFork *fork) {
    RadValueCopySource src;
    src.fork = fork;
    src.use_fork = true;
    return rad_value_deep_copy_with_source(v, &src);
}

#ifdef RAD_SCRATCH_ARENA
static RadValue rad_scratch_promote(RadValue v) {
    (void)v;
    return v;
}
#endif /* RAD_SCRATCH_ARENA */

/* ========== format_value(value, spec) ========== */

#define RAD_FMT_MAX_WIDTH 10000

typedef struct {
    char fill;
    char align;   /* '<', '>', '^', or 0 for default */
    char sign;    /* '+', '-', ' ', or 0 */
    int alt;
    int zero_pad;
    int width;    /* -1 = unset */
    int precision; /* -1 = unset */
    char type;    /* 'd','f','e','E','x','X','o','b','s','%', or 0 */
} RadFormatSpec;

static RadFormatSpec rad_parse_format_spec(const char *s, int64_t slen) {
    RadFormatSpec sp = {' ', 0, 0, 0, 0, -1, -1, 0};
    int i = 0;
    if (slen >= 2 && (s[1] == '<' || s[1] == '>' || s[1] == '^')) {
        sp.fill = s[0]; sp.align = s[1]; i = 2;
    } else if (slen >= 1 && (s[0] == '<' || s[0] == '>' || s[0] == '^')) {
        sp.align = s[0]; i = 1;
    }
    if (i < slen && (s[i] == '+' || s[i] == '-' || s[i] == ' ')) { sp.sign = s[i]; i++; }
    if (i < slen && s[i] == '#') { sp.alt = 1; i++; }
    if (i < slen && s[i] == '0') {
        if ((i + 1 < slen && s[i+1] >= '0' && s[i+1] <= '9') || (sp.align == 0 && i + 1 < slen)) {
            sp.zero_pad = 1; i++;
        }
    }
    if (i < slen && s[i] >= '0' && s[i] <= '9') {
        sp.width = 0;
        while (i < slen && s[i] >= '0' && s[i] <= '9') { sp.width = sp.width * 10 + (s[i] - '0'); i++; }
        if (sp.width > RAD_FMT_MAX_WIDTH) sp.width = RAD_FMT_MAX_WIDTH;
    }
    if (i < slen && s[i] == '.') {
        i++; sp.precision = 0;
        while (i < slen && s[i] >= '0' && s[i] <= '9') { sp.precision = sp.precision * 10 + (s[i] - '0'); i++; }
        if (sp.precision > RAD_FMT_MAX_WIDTH) sp.precision = RAD_FMT_MAX_WIDTH;
    }
    if (i < slen) { sp.type = s[i]; i++; }
    return sp;
}

static RadValue rad_apply_padding(const char *s, int slen, const RadFormatSpec *sp) {
    if (sp->width < 0 || slen >= sp->width) return rad_make_str(s);
    int w = sp->width;
    int pad = w - slen;
    char al = sp->align ? sp->align : (sp->zero_pad ? '>' : '<');
    char stack_buf[256];
    char *buf;
    size_t need = (size_t)(w + 1);
    if (need <= sizeof(stack_buf)) {
        buf = stack_buf;
    } else {
        buf = (char *)malloc(need);
        if (!buf) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
    }
    if (al == '>') {
        for (int j = 0; j < pad; j++) buf[j] = sp->fill;
        memcpy(buf + pad, s, slen);
    } else if (al == '<') {
        memcpy(buf, s, slen);
        for (int j = 0; j < pad; j++) buf[slen + j] = sp->fill;
    } else { /* '^' */
        int left = pad / 2, right = pad - left;
        for (int j = 0; j < left; j++) buf[j] = sp->fill;
        memcpy(buf + left, s, slen);
        for (int j = 0; j < right; j++) buf[left + slen + j] = sp->fill;
    }
    buf[w] = '\0';
    RadValue result = rad_make_str(buf);
    if (need > sizeof(stack_buf)) free(buf);
    return result;
}

static RadValue rad_format_int(int64_t val, const RadFormatSpec *sp) {
    char tmp[128];
    int tlen = 0;
    char ty = sp->type ? sp->type : 'd';

    switch (ty) {
    case 'd': case 's': tlen = snprintf(tmp, sizeof(tmp), "%" PRId64, val); break;
    case 'b': {
        uint64_t uv = (uint64_t)val;
        int bi = 127; tmp[bi] = '\0';
        if (uv == 0) { tmp[--bi] = '0'; }
        else { while (uv) { tmp[--bi] = '0' + (uv & 1); uv >>= 1; } }
        if (sp->alt) { tmp[--bi] = 'b'; tmp[--bi] = '0'; }
        tlen = 127 - bi;
        memmove(tmp, tmp + bi, tlen + 1);
        break;
    }
    case 'o': {
        if (sp->alt) tlen = snprintf(tmp, sizeof(tmp), "0o%" PRIo64, (uint64_t)val);
        else tlen = snprintf(tmp, sizeof(tmp), "%" PRIo64, (uint64_t)val);
        break;
    }
    case 'x': {
        if (sp->alt) tlen = snprintf(tmp, sizeof(tmp), "0x%" PRIx64, (uint64_t)val);
        else tlen = snprintf(tmp, sizeof(tmp), "%" PRIx64, (uint64_t)val);
        break;
    }
    case 'X': {
        if (sp->alt) tlen = snprintf(tmp, sizeof(tmp), "0X%" PRIX64, (uint64_t)val);
        else tlen = snprintf(tmp, sizeof(tmp), "%" PRIX64, (uint64_t)val);
        break;
    }
    case 'f': case 'F': {
        int prec = sp->precision >= 0 ? sp->precision : 6;
        tlen = snprintf(tmp, sizeof(tmp), "%.*f", prec, (double)val);
        break;
    }
    case 'e': {
        int prec = sp->precision >= 0 ? sp->precision : 6;
        tlen = snprintf(tmp, sizeof(tmp), "%.*e", prec, (double)val);
        break;
    }
    case 'E': {
        int prec = sp->precision >= 0 ? sp->precision : 6;
        tlen = snprintf(tmp, sizeof(tmp), "%.*E", prec, (double)val);
        break;
    }
    case '%': {
        int prec = sp->precision >= 0 ? sp->precision : 6;
        tlen = snprintf(tmp, sizeof(tmp), "%.*f%%", prec, (double)val * 100.0);
        break;
    }
    default: tlen = snprintf(tmp, sizeof(tmp), "%" PRId64, val); break;
    }
    if (tlen >= (int)sizeof(tmp)) tlen = (int)sizeof(tmp) - 1;

    if (ty == 'd' || ty == 'b' || ty == 'o' || ty == 'x' || ty == 'X' || ty == 'f' || ty == 'e' || ty == 'E') {
        if (sp->sign == '+' && val >= 0 && tlen + 1 < (int)sizeof(tmp)) {
            memmove(tmp + 1, tmp, tlen + 1); tmp[0] = '+'; tlen++;
        } else if (sp->sign == ' ' && val >= 0 && tlen + 1 < (int)sizeof(tmp)) {
            memmove(tmp + 1, tmp, tlen + 1); tmp[0] = ' '; tlen++;
        }
    }

    if (sp->zero_pad && sp->align == 0 && sp->width > 0 && tlen < sp->width) {
        int w = sp->width;
        int pfx = 0;
        if (tmp[0] == '+' || tmp[0] == '-' || tmp[0] == ' ') pfx = 1;
        else if (tlen >= 2 && tmp[0] == '0' && (tmp[1] == 'x' || tmp[1] == 'X' || tmp[1] == 'b' || tmp[1] == 'o')) pfx = 2;
        int zeros = w - tlen;
        char stack_buf2[256];
        char *buf2;
        size_t need2 = (size_t)(w + 1);
        if (need2 <= sizeof(stack_buf2)) {
            buf2 = stack_buf2;
        } else {
            buf2 = (char *)malloc(need2);
            if (!buf2) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
        }
        memcpy(buf2, tmp, pfx);
        for (int j = 0; j < zeros; j++) buf2[pfx + j] = '0';
        memcpy(buf2 + pfx + zeros, tmp + pfx, tlen - pfx);
        buf2[w] = '\0';
        RadValue result = rad_make_str(buf2);
        if (need2 > sizeof(stack_buf2)) free(buf2);
        return result;
    }

    RadFormatSpec num_sp = *sp;
    if (num_sp.align == 0) num_sp.align = '>';
    return rad_apply_padding(tmp, tlen, &num_sp);
}

static RadValue rad_format_float(double val, const RadFormatSpec *sp) {
    char tmp[128];
    int tlen = 0;
    char ty = sp->type ? sp->type : 'f';
    int prec = sp->precision >= 0 ? sp->precision : 6;

    switch (ty) {
    case 'f': case 'F': tlen = snprintf(tmp, sizeof(tmp), "%.*f", prec, val); break;
    case 'e': tlen = snprintf(tmp, sizeof(tmp), "%.*e", prec, val); break;
    case 'E': tlen = snprintf(tmp, sizeof(tmp), "%.*E", prec, val); break;
    case '%': tlen = snprintf(tmp, sizeof(tmp), "%.*f%%", prec, val * 100.0); break;
    case 'd': tlen = snprintf(tmp, sizeof(tmp), "%" PRId64, (int64_t)val); break;
    case 's': tlen = snprintf(tmp, sizeof(tmp), "%g", val); break;
    default: tlen = snprintf(tmp, sizeof(tmp), "%.*f", prec, val); break;
    }
    if (tlen >= (int)sizeof(tmp)) tlen = (int)sizeof(tmp) - 1;

    if (ty == 'f' || ty == 'F' || ty == 'e' || ty == 'E' || ty == '%' || ty == 'd') {
        if (sp->sign == '+' && val >= 0.0 && !isnan(val) && tlen + 1 < (int)sizeof(tmp)) {
            memmove(tmp + 1, tmp, tlen + 1); tmp[0] = '+'; tlen++;
        } else if (sp->sign == ' ' && val >= 0.0 && !isnan(val) && tlen + 1 < (int)sizeof(tmp)) {
            memmove(tmp + 1, tmp, tlen + 1); tmp[0] = ' '; tlen++;
        }
    }

    if (sp->zero_pad && sp->align == 0 && sp->width > 0 && tlen < sp->width) {
        int w = sp->width;
        int pfx = (tmp[0] == '+' || tmp[0] == '-' || tmp[0] == ' ') ? 1 : 0;
        int zeros = w - tlen;
        char stack_buf2[256];
        char *buf2;
        size_t need2 = (size_t)(w + 1);
        if (need2 <= sizeof(stack_buf2)) {
            buf2 = stack_buf2;
        } else {
            buf2 = (char *)malloc(need2);
            if (!buf2) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
        }
        memcpy(buf2, tmp, pfx);
        for (int j = 0; j < zeros; j++) buf2[pfx + j] = '0';
        memcpy(buf2 + pfx + zeros, tmp + pfx, tlen - pfx);
        buf2[w] = '\0';
        RadValue result = rad_make_str(buf2);
        if (need2 > sizeof(stack_buf2)) free(buf2);
        return result;
    }

    RadFormatSpec num_sp = *sp;
    if (num_sp.align == 0) num_sp.align = '>';
    return rad_apply_padding(tmp, tlen, &num_sp);
}

static RadValue rad_format_str(const char *s, int64_t slen, const RadFormatSpec *sp) {
    char stack_trunc[256];
    char *truncated = NULL;
    int actual_len = (int)slen;
    if (sp->precision >= 0 && slen > sp->precision) {
        actual_len = sp->precision;
        size_t need = (size_t)(actual_len + 1);
        if (need <= sizeof(stack_trunc)) {
            truncated = stack_trunc;
        } else {
            truncated = (char *)malloc(need);
            if (!truncated) { fprintf(stderr, "rad runtime: out of memory\n"); exit(1); }
        }
        memcpy(truncated, s, actual_len);
        truncated[actual_len] = '\0';
        s = truncated;
    }
    RadFormatSpec sp2 = *sp;
    if (sp2.align == 0) sp2.align = '<';
    RadValue result = rad_apply_padding(s, actual_len, &sp2);
    if (truncated && truncated != stack_trunc) free(truncated);
    return result;
}

RAD_API RadValue rad_format_value(RadValue val, RadValue spec_v) {
    if (spec_v.tag != RV_STR || spec_v.as.str.len == 0) return str(val);
    RadFormatSpec sp = rad_parse_format_spec(spec_v.as.str.data, spec_v.as.str.len);

    switch (val.tag) {
    case RV_INT: return rad_format_int(val.as.i, &sp);
    case RV_FLOAT: return rad_format_float(val.as.f, &sp);
    case RV_STR: return rad_format_str(val.as.str.data, val.as.str.len, &sp);
    default: {
        RadValue sv = str(val);
        return rad_format_str(sv.as.str.data, sv.as.str.len, &sp);
    }
    }
}

/* Debug-only: next entity id allocator cursor (monotonic while ids not reused). */
#ifdef RAD_REPRO_METRICS
RAD_API int64_t rad_repro_metrics_next_entity(void) { return rad_next_entity; }
#endif

#endif
