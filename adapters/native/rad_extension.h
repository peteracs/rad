#ifndef RAD_EXTENSION_H
#define RAD_EXTENSION_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define RAD_EXTENSION_ABI_VERSION 3u

#if defined(_WIN32)
#define RAD_EXPORT __declspec(dllexport)
#else
#define RAD_EXPORT __attribute__((visibility("default")))
#endif

typedef uint64_t (*RadNativeFn)(const uint64_t *args, size_t argc);

typedef struct RadExtensionDescriptor {
    uint32_t abi_version;
    const char *extension_id;
    const char *extension_version;
    const char *abi_contract_json;
} RadExtensionDescriptor;

typedef struct RadNativeFunctionDecl {
    const char *name;
    RadNativeFn function;
    uint32_t arity;
    const char *const *effects;
    size_t effect_count;
    const char *signature;
    bool deterministic;
    bool replayable;
} RadNativeFunctionDecl;

typedef struct RadPluginApi {
    void *context;
    void (*register_fn)(void *context, const RadNativeFunctionDecl *declaration);
    uint64_t (*make_nil)(void);
    uint64_t (*make_int)(int64_t value);
    uint64_t (*make_float)(double value);
    uint64_t (*make_bool)(bool value);
    uint64_t (*make_string)(const char *value);
    uint64_t (*make_host_handle)(const char *type_name, uint64_t token);
    bool (*as_int)(uint64_t value, int64_t *out);
    bool (*as_float)(uint64_t value, double *out);
    bool (*as_bool)(uint64_t value, bool *out);
    const char *(*as_string_ptr)(uint64_t value);
    size_t (*as_string_len)(uint64_t value);
    bool (*as_host_handle)(uint64_t value, uint64_t *out);
    void (*set_error)(const char *message);
} RadPluginApi;

/*
 * Required exports:
 *   RAD_EXPORT const RadExtensionDescriptor *rad_extension_descriptor(void);
 *   RAD_EXPORT void rad_extension_init(const RadPluginApi *api);
 *
 * `abi_contract_json` must be a version-1 JSON C-layout contract. Every
 * function declaration must list its logical signature and complete semantic effects using:
 *   reads:Type, writes:Type, emits:Event, io, async
 */

#endif
