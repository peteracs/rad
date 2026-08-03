//! One authoritative opcode-effect classification for causal execution.
//!
//! RFC-0002 constraint closure analysis can reuse this table instead of
//! growing a second, subtly different purity model.

use crate::opcode::Op;
use crate::value::Builtin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpcodeEffect {
    /// Pure computation, immutable reads, or transaction-local state.
    CausalSafe,
    /// Local mutation whose executor must reject captured-cell targets.
    LocalMutation,
    /// Dynamic call; the callee/builtin/native path performs the final check.
    DynamicCall,
    /// A frame exit is legal only for a callee, never the settlement owner.
    FrameExit,
    /// RFC-0001 transaction control and transient causal buffers.
    SettlementKernel,
    /// Durable or externally observable VM mutation.
    Forbidden(&'static str),
}

pub(crate) fn opcode_effect(op: Op) -> OpcodeEffect {
    use Op::*;
    match op {
        SetUpvalue => OpcodeEffect::Forbidden("captured-state mutation"),
        DefGlobal | SetGlobal => OpcodeEffect::Forbidden("global-state mutation"),
        AsyncCall | Await | Yield => OpcodeEffect::Forbidden("async/task state"),
        EcsSet | EcsSpawn | InitResource | LogicalStore => {
            OpcodeEffect::Forbidden("direct world mutation")
        }
        Emit | EmitAfter => OpcodeEffect::Forbidden("event emission"),
        RunSystem | RunSchedule | RunScheduleSerial => OpcodeEffect::Forbidden("system scheduling"),
        Print => OpcodeEffect::Forbidden("output"),
        Snapshot | Rollback => OpcodeEffect::Forbidden("timeline mutation"),
        OnceGuardPass => OpcodeEffect::Forbidden("handler state mutation"),
        Halt => OpcodeEffect::Forbidden("VM termination"),

        SetLocal | MoveLocal | IncLocal | ForRangeNext => OpcodeEffect::LocalMutation,

        // A local slot establishes locality of the pointer, not uniqueness of
        // the referenced GC object.  Every true in-place/interior mutation is
        // therefore forbidden until causal regions have allocation epochs and
        // alias-aware ownership.  Functional SetIndex/SetField remain safe:
        // they produce replacement values rather than modifying an alias.
        ListPushLocal
        | ListSetLocal
        | BitsetSetInplace
        | BitsetClearInplace
        | BufferAppendInplace
        | ByteBufSetU8Inplace
        | ByteBufSetU32LeInplace
        | ByteBufSetI32LeInplace
        | IterNext => OpcodeEffect::Forbidden("interior heap mutation"),

        Call => OpcodeEffect::DynamicCall,
        Return | Try => OpcodeEffect::FrameExit,
        BeginSettlement | EndSettlement | ProposeIntent | StageCandidate => {
            OpcodeEffect::SettlementKernel
        }

        Const | Pop | PopN | Dup | Add | Sub | Mul | Div | Mod | Neg | Eq | Neq | Lt | Gt | Lte
        | Gte | Not | And | Or | GetGlobal | GetLocal | GetLocal2 | GetUpvalue | Jump
        | JumpIfFalse | JumpBack | MakeList | MakeComp | GetField | SetField | GetIndex
        | SetIndex | EcsGet | EcsHas | EcsQuery | MakeState | Transition | MakeVariant
        | MatchState | IsVariant | Pipe | Len | TypeOf | Break | Closure | MakeMap
        | GetFieldSlot | SetFieldSlot | MakeCompSlot | QueryFilter | QueryProject | MakeTuple
        | Unpack | GetIter | VecAdd | VecSub | VecMul | VecDiv | VecMod | VecNeg | VecNot
        | VecEq | VecNeq | VecLt | VecGt | VecLte | VecGte | VecFilter | VecSelect | LoadColumn
        | VecBroadcast | PopCheckErr | LogicalLoad | MaterializeAoS | ConcatN | BitAnd | BitOr
        | BitXor | ListGetLocal | ListGetLL | EqJF | NeqJF | LtJF | LteJF | GtJF | GteJF
        | EqConst | NeqConst | EqConstJF | NeqConstJF | ConstArith | Shl | Shr | BitNot => {
            OpcodeEffect::CausalSafe
        }
    }
}

pub(crate) fn forbidden_builtin_effect(builtin: Builtin) -> Option<String> {
    if matches!(
        builtin,
        Builtin::SysArgs
            | Builtin::DebugTrace
            | Builtin::TraceId
            | Builtin::SandboxInput
            | Builtin::SandboxOutput
            | Builtin::SandboxLastOutput
            | Builtin::SandboxLastFuel
    ) {
        return Some("host/output state".to_string());
    }
    let effects = crate::builtins::builtin_effect(builtin.name());
    if effects.is_pure() || effects.is_readonly() {
        None
    } else {
        Some(format!("{} effect", effects))
    }
}
