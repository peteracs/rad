use crate::bytecode_effects::{forbidden_builtin_effect, EffectBoundary};

struct BuiltinApiMetadata {
    category: &'static str,
    errors: &'static str,
    determinism: &'static str,
    complexity: &'static str,
    allocation: &'static str,
    native: &'static str,
    wasm: &'static str,
    sandbox: &'static str,
    transaction: String,
    post_commit: String,
    settlement: String,
}

fn builtin_api_metadata(builtin: Builtin, signature: &str) -> BuiltinApiMetadata {
    BuiltinApiMetadata {
        category: builtin_api_category(builtin),
        errors: builtin_error_contract(signature),
        determinism: builtin_determinism(builtin),
        complexity: builtin_complexity(builtin),
        allocation: builtin_allocation(builtin),
        native: "available",
        wasm: if builtin_is_native_only(builtin) {
            "unavailable"
        } else {
            "available"
        },
        sandbox: if crate::sandbox::builtin_allowed_in_sandbox(builtin) {
            "capability-checked"
        } else {
            "denied"
        },
        transaction: boundary_contract(EffectBoundary::Transaction, builtin),
        post_commit: boundary_contract(EffectBoundary::PostCommit, builtin),
        settlement: boundary_contract(EffectBoundary::Settlement, builtin),
    }
}

fn boundary_contract(boundary: EffectBoundary, builtin: Builtin) -> String {
    match forbidden_builtin_effect(boundary, builtin) {
        Some(reason) => format!("denied: {reason}"),
        None => "allowed".to_string(),
    }
}

fn builtin_error_contract(signature: &str) -> &'static str {
    if signature.contains("Result<") {
        "recoverable domain failure is Result::Err; invalid arity/type/capability is a runtime error"
    } else if signature.contains("Option<") || signature.contains(" | nil") {
        "ordinary absence is Option::None or nil; invalid arity/type/capability is a runtime error"
    } else {
        "invalid arity, type, range, capability, platform, or runtime state is a runtime error"
    }
}

fn builtin_determinism(builtin: Builtin) -> &'static str {
    use Builtin::*;
    match builtin {
        RandInt | RandFloat | RandBool | RandSeed | SimulateSeeded | SimulatePar
        | SimulateMany => "deterministic under the recorded VM seed",
        SysArgs | Input | Readline | ReadStdinAll | ReadFile | WriteFile | AppendFile
        | FileExists | RemoveFile | ListDir | CreateDir | RemoveDir | ReadFileBytes
        | WriteFileBytes | HttpGet | HttpPost | HttpPostJson | HttpRequest | TcpConnect
        | TcpListen | TcpAccept | TcpAcceptTimeout | TcpRead | TcpWrite | TcpClose
        | UdpBind | UdpRecvFrom | UdpRecvFromTimeout | UdpRecvFromBytes
        | UdpRecvFromBytesTimeout | UdpRecvByteBuf | UdpRecvByteBufTimeout | UdpSendTo
        | UdpSendToBytes | UdpSendByteBuf | UdpClose | NowUnixS | NowUnixMs | Clock
        | LoadExtension | HostTry | HostTry0 | SleepMs =>
            "external result/effect must be recorded for deterministic replay",
        Print | Eprint | WriteStdout | WriteStderr | FlushStdout | Log | Metric
        | TraceId | DebugTrace | SandboxOutput =>
            "deterministic RAD result with an ordered output/telemetry effect",
        _ => "deterministic for equal RAD state and arguments",
    }
}

fn builtin_complexity(builtin: Builtin) -> &'static str {
    use Builtin::*;
    match builtin {
        Lookup | GetEntity | RequireEntity | NameOf | IdOf | ViewRevision | ByteAt
        | ByteLen | ByteBufLen | ByteBufGet | ByteBufSetU8 | ByteBufSetU32Le
        | ByteBufSetI32Le | ByteBufGetU32Le | ByteBufGetI32Le | BitsetHas
        | BitsetSet | BitsetClear => "O(1)",
        LowerBound | UpperBound | First | Last | Next | Previous => "O(log n)",
        LookupAll | VisitView | ChangesSince | RecentEvents => "O(k)",
        Entities | QueryWhere | QueryMap | QueryCount => "O(n) over the selected population",
        Sort | SortBy => "O(n log n)",
        Range => "O(k) for ordered-index range; O(n) for numeric range materialization",
        _ => "no stronger asymptotic guarantee than the documented input/output size",
    }
}

fn builtin_allocation(builtin: Builtin) -> &'static str {
    use Builtin::*;
    match builtin {
        Lookup | GetEntity | RequireEntity | NameOf | IdOf | ViewRevision | QueryCount
        | VisitView | LowerBound | UpperBound | First | Last | Next | Previous | ByteLen
        | ByteBufLen | ByteBufGet | ByteBufSetU8 | ByteBufSetU32Le | ByteBufSetI32Le
        | ByteBufGetU32Le | ByteBufGetI32Le | BitsetHas | BitsetSet | BitsetClear
        | Popcount | Ctz | Shl | Shr | Abs | Sign | Min | Max | IntDiv | Round | Floor
        | Ceil | Sqrt | Pow | Clamp => "no collection result; enclosing runtime contract is authoritative",
        Entities | LookupAll | QueryWhere | QueryMap | Sort | SortBy | Range | ChangesSince
        | RecentEvents | Map | Filter | FlatMap | GroupBy | Zip | Enumerate | Keys | Values
        | Entries | Chars | Split | ForkToBytes | ForkFromBytes | ForkDelta | SaveWorld
        | JsonStringify | JsonParse | EncodeLe | EncodeBe | DecodeLe | DecodeBe =>
            "allocates a result proportional to output size",
        _ => "not statically guaranteed allocation-free",
    }
}

fn builtin_is_native_only(builtin: Builtin) -> bool {
    use Builtin::*;
    matches!(
        builtin,
        Input
            | Readline
            | ReadStdinAll
            | ReadFile
            | WriteFile
            | AppendFile
            | FileExists
            | RemoveFile
            | ListDir
            | CreateDir
            | RemoveDir
            | ReadFileBytes
            | WriteFileBytes
            | HttpGet
            | HttpPost
            | HttpPostJson
            | HttpRequest
            | TcpConnect
            | TcpListen
            | TcpAccept
            | TcpAcceptTimeout
            | TcpRead
            | TcpWrite
            | TcpClose
            | UdpBind
            | UdpRecvFrom
            | UdpRecvFromTimeout
            | UdpRecvFromBytes
            | UdpRecvFromBytesTimeout
            | UdpRecvByteBuf
            | UdpRecvByteBufTimeout
            | UdpSendTo
            | UdpSendToBytes
            | UdpSendByteBuf
            | UdpClose
            | LoadExtension
    )
}

fn builtin_api_category(builtin: Builtin) -> &'static str {
    use Builtin::*;
    match builtin {
        Print | SysArgs | ReadFile | WriteFile | HttpGet | NowUnixS | NowUnixMs | Input
        | Readline | GcCollect | Eprint | WriteStdout
        | WriteStderr | ReadStdinAll | FlushStdout | SleepMs | AppendFile | FileExists
        | RemoveFile | ListDir | CreateDir | RemoveDir | ReadFileBytes | WriteFileBytes
        | HttpPost | HttpPostJson | HttpRequest | TcpConnect | TcpListen | TcpAccept
        | TcpAcceptTimeout | TcpRead | TcpWrite | TcpClose | UdpBind | UdpRecvFrom
        | UdpRecvFromTimeout | UdpRecvFromBytes | UdpRecvFromBytesTimeout | UdpRecvByteBuf
        | UdpRecvByteBufTimeout | UdpSendTo | UdpSendToBytes | UdpSendByteBuf | UdpClose
        | Log | Metric | TraceId | DebugTrace => "host-io-network",

        LoadExtension | HostTry | HostTry0 => "ffi",

        Push | Pop | PopLast | DropLast | Sort | Reverse | Slice | Map | Filter | Reduce
        | Range | Keys | Contains | Entries | Merge | RemoveKey | GroupBy | Append | Extend
        | Zip | FlatMap | Values | SortBy | Enumerate | Find | MaxBy | MinBy | Filled
        | SetAt | Sum | Product | GetOr | IndexOf | Any | All | DropFirst => "collections",

        Format | Split | Join | Trim | Replace | StartsWith | EndsWith | Chr | Ord | Chars
        | ToUpper | ToLower | RegexIsMatch | RegexFind | ByteAt | SubstringBytes | ByteLen
        | FormatValue | ToFixed | JsonStringify | JsonParse | Str => "text-json",

        BitsetNew | BitsetSet | BitsetHas | BitsetClear | BufferNew | BufferAppend
        | BufferToStr | ByteBufNew | ByteBufLen | ByteBufGet | ByteBufSetU8
        | ByteBufSetU32Le | ByteBufSetI32Le | ByteBufGetU32Le | ByteBufGetI32Le
        | ByteBufToList | ByteBufFromList => "buffers-bitsets",

        SizeOf | OffsetOf | DecodeLe | DecodeBe | EncodeLe | EncodeBe => "native-layout",

        Get | ReadField | Lookup | LookupAll | Set | WriteField | Has | Spawn | GetEntity
        | RequireEntity | Remove | Despawn | Entities | VisitView | GetResource | SetResource
        | Transition | Require | RequireAll | NameOf | IdOf | QueryWhere | QueryMap
        | QueryCount | WithField | Res => "ecs-resources",

        ViewRevision | ChangesSince | LowerBound | UpperBound | First | Last | Next
        | Previous => "queries-indexes-views",

        EnterPhase | MarkPhase | AssertTrace | RecentEvents | FlushEvents => "events-phases",

        WhyInView | WhyNotInView | WhyField | WhyRemoved | WhyMissing
        | WhyRevisionChanged | WhyRevisionDidNotChange | Why | WhyResource => "provenance",

        Fork | Simulate | Commit | Clock | Peek | PeekResource | SimulatePar | SandboxRun
        | SandboxInput | SandboxOutput | SandboxLastOutput | SandboxLastFuel | SimulateMany
        | SimulateSeeded | ForkWith | ForkSeed | Diff | AssertOnlyChanged | MergeForks
        | MergeForksWith => "transactions-speculation",

        SaveWorld | LoadWorld | TryLoadWorld | WorldDigest | SchemaDigest | ForkToBytes
        | ForkFromBytes | ForkDelta | ForkApply => "persistence-wire-replay",

        RandInt | RandFloat | RandBool | RandSeed | GenInt | GenFloat | GenStr | GenBool
        | GenList | Assert | AssertEq | ModelCheck => "testing-models",

        BaseFact | CandidateFact | InsertFact | RemoveFact | ReplaceFactBy | WhyFact =>
            "relations",

        Len | TypeOf | VariantOf | Int | Float | Abs | Sign | Min | Max | Unwrap
        | Expect | TryInt | TryFloat | IntDiv | UnwrapOr | IsSome | IsNone | MapOr
        | Round | Floor | Ceil | Sqrt | Pow | Popcount | Ctz | Shl | Shr
        | Clamp => "values-control",
    }
}
