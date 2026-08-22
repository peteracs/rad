#include "rad_extension.h"

#ifndef RISK_FIXTURE_MODE
#define RISK_FIXTURE_MODE 0
#endif

static RadPluginApi HOST;
static uint64_t PROBE_COUNT;

#if RISK_FIXTURE_MODE == 2
static const char ABI_CONTRACT[] =
    "{\"contract_version\":1,\"calling_convention\":\"stdcall\",\"types\":[]}";
#elif RISK_FIXTURE_MODE == 3
static const char ABI_CONTRACT[] =
    "{\"contract_version\":1,\"calling_convention\":\"C\",\"types\":[{\"name\":\"RiskInput\",\"size\":40,\"alignment\":8,\"fields\":[{\"name\":\"customer\",\"type_name\":\"CustomerId\",\"offset\":0,\"size\":8}]}]}";
#elif RISK_FIXTURE_MODE == 4
static const char ABI_CONTRACT[] =
    "{\"contract_version\":1,\"calling_convention\":\"C\",\"types\":[{\"name\":\"RiskInput\",\"size\":48,\"alignment\":8,\"fields\":[{\"name\":\"customer\",\"type_name\":\"CustomerId\",\"offset\":4,\"size\":8}]}]}";
#elif RISK_FIXTURE_MODE == 5
static const char ABI_CONTRACT[] =
    "{\"contract_version\":1,\"calling_convention\":\"C\",\"types\":[{\"name\":\"RiskInput\",\"size\":48,\"alignment\":8,\"fields\":[{\"name\":\"customer\",\"type_name\":\"TransactionId\",\"offset\":0,\"size\":8}]}]}";
#elif RISK_FIXTURE_MODE == 6
static const char ABI_CONTRACT[] =
    "{\"contract_version\":1,\"calling_convention\":\"C\",\"types\":[{\"name\":\"Broken\",\"size\":8,\"alignment\":8,\"fields\":[{\"name\":\"a\",\"type_name\":\"u64\",\"offset\":0,\"size\":8},{\"name\":\"b\",\"type_name\":\"u32\",\"offset\":4,\"size\":4}]}]}";
#else
static const char ABI_CONTRACT[] =
    "{\"contract_version\":1,\"calling_convention\":\"C\",\"types\":[]}";
#endif

static const RadExtensionDescriptor DESCRIPTOR = {
#if RISK_FIXTURE_MODE == 1
    2u,
#else
    RAD_EXTENSION_ABI_VERSION,
#endif
    "rad.dogfood.riskbridge.model",
    "1.0.0",
    ABI_CONTRACT,
};

static uint64_t probe(const uint64_t *args, size_t argc) {
    (void)args;
    if (argc != 0) {
        HOST.set_error("fixture probe expects no arguments");
        return HOST.make_nil();
    }
#if RISK_FIXTURE_MODE == 8
    *(volatile int *)0 = 1;
#elif RISK_FIXTURE_MODE == 9
    for (;;) {}
#elif RISK_FIXTURE_MODE == 10
    return HOST.make_int((int64_t)++PROBE_COUNT);
#else
    return HOST.make_string("riskbridge-generation-17-deterministic");
#endif
}

static uint64_t score(const uint64_t *args, size_t argc) {
    (void)args;
    (void)argc;
#if RISK_FIXTURE_MODE == 12
    return HOST.make_string("not-json");
#elif RISK_FIXTURE_MODE == 13
    return HOST.make_string("[0,0,192,127,0,0,0,0,0,0,0,0,0,0,0,0,17,0,0,0,0,0,0,0]");
#elif RISK_FIXTURE_MODE == 14
    return HOST.make_string("[0,0,0,0,0,0,0,0,16,0,0,0,0,0,0,0,17,0,0,0,0,0,0,0]");
#elif RISK_FIXTURE_MODE == 15
    return HOST.make_string("[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,17,0,0,0,9,0,0,0]");
#elif RISK_FIXTURE_MODE == 16
    HOST.set_error("typed fixture host failure");
    return HOST.make_nil();
#elif RISK_FIXTURE_MODE == 17
    return HOST.make_host_handle("RiskModelSession", 42);
#else
    return HOST.make_string("[]");
#endif
}

RAD_EXPORT const RadExtensionDescriptor *rad_extension_descriptor(void) {
    return &DESCRIPTOR;
}

RAD_EXPORT void rad_extension_init(const RadPluginApi *api) {
    static const char *IO_EFFECTS[] = { "io" };
    static const RadNativeFunctionDecl PROBE = {
        "risk_determinism_probe",
        probe,
        0,
        NULL,
        0,
        "()->str",
        true,
        true,
    };
    RadNativeFunctionDecl score_decl = {
        "score_transaction",
        score,
        1,
#if RISK_FIXTURE_MODE == 7
        IO_EFFECTS,
        1,
#else
        NULL,
        0,
#endif
        "(RiskInput)->RiskOutput",
        true,
        true,
    };
    HOST = *api;
    api->register_fn(api->context, &PROBE);
#if RISK_FIXTURE_MODE != 11
    api->register_fn(api->context, &score_decl);
#endif
}
