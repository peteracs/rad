//! One authoritative opcode and builtin boundary policy.
//!
//! Settlement, ordinary transaction, and post-commit execution all consume
//! this module. Adding an opcode therefore requires one semantic
//! classification and cannot silently create three different firewalls.

use crate::opcode::Op;
use crate::value::Builtin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EffectBoundary {
    Settlement,
    Transaction,
    PostCommit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpcodeEffect {
    Pure,
    LocalMutation,
    InteriorMutation,
    DynamicCall,
    FrameExit,
    CapturedStateMutation,
    GlobalStateMutation,
    AsyncTaskState,
    TransactionalWorldMutation,
    DeclarationStateMutation,
    EventEmission,
    StateTransition,
    SystemScheduling,
    Output,
    Snapshot,
    Rollback,
    HandlerStateMutation,
    VmTermination,
    SettlementKernel,
    BeginTransaction,
    TransactionProgress,
    EndPostCommit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoundaryViolation {
    SettlementEffect(&'static str),
    AuthoritativeStateMutation,
    SettlementKernel,
    GlobalOrCapturedState,
    AsyncBeforeCommit,
    EventBeforeCommit,
    ScheduleBeforeCommit,
    OutputBeforeCommit,
    TimelineOrDeclarationState,
    InteriorMutation,
    VmLifecycleState,
    NestedTransaction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpcodePolicy {
    Allow,
    CheckOwnerFrameExit,
    Reject(BoundaryViolation),
}

fn opcode_effect(op: Op) -> OpcodeEffect {
    use Op::*;
    match op {
        SetUpvalue => OpcodeEffect::CapturedStateMutation,
        DefGlobal | SetGlobal => OpcodeEffect::GlobalStateMutation,
        AsyncCall | Await | Yield => OpcodeEffect::AsyncTaskState,
        EcsSet | EcsSpawn | EcsWriteField | RunViewKernel | LogicalStore => {
            OpcodeEffect::TransactionalWorldMutation
        }
        InitResource => OpcodeEffect::DeclarationStateMutation,
        Emit | EmitAfter | EmitSync | EmitPhase => OpcodeEffect::EventEmission,
        Transition => OpcodeEffect::StateTransition,
        RunSystem | RunSchedule | RunScheduleSerial => OpcodeEffect::SystemScheduling,
        Print => OpcodeEffect::Output,
        Snapshot => OpcodeEffect::Snapshot,
        Rollback => OpcodeEffect::Rollback,
        OnceGuardPass => OpcodeEffect::HandlerStateMutation,
        Halt => OpcodeEffect::VmTermination,

        SetLocal | MoveLocal | IncLocal | ForRangeNext => OpcodeEffect::LocalMutation,

        // A local slot establishes locality of the pointer, not uniqueness of
        // the referenced GC object. True in-place/interior mutation is not
        // safe in a staged transaction or settlement.
        ListPushLocal
        | ListSetLocal
        | BitsetSetInplace
        | BitsetClearInplace
        | BufferAppendInplace
        | ByteBufSetU8Inplace
        | ByteBufSetU32LeInplace
        | ByteBufSetI32LeInplace
        | IterNext => OpcodeEffect::InteriorMutation,

        Call => OpcodeEffect::DynamicCall,
        Return | Try => OpcodeEffect::FrameExit,
        BeginSettlement
        | EndSettlement
        | ProposeIntent
        | StageCandidate
        | ReadBaseComponent
        | ReadCandidateComponent
        | RequireConstraint => OpcodeEffect::SettlementKernel,
        BeginTransaction => OpcodeEffect::BeginTransaction,
        CheckTransaction | EndTransaction => OpcodeEffect::TransactionProgress,
        EndPostCommit => OpcodeEffect::EndPostCommit,

        Const | Pop | PopN | Dup | Add | Sub | Mul | Div | Mod | Neg | Eq | Neq | Lt | Gt | Lte
        | Gte | Not | And | Or | GetGlobal | GetLocal | GetLocal2 | GetUpvalue | Jump
        | JumpIfFalse | JumpBack | MakeList | MakeComp | GetField | SetField | GetIndex
        | SetIndex | EcsGet | EcsReadField | EcsHas | EcsQuery | MakeState | MakeVariant
        | MatchState | IsVariant | Pipe | Len | TypeOf | Break | Closure | MakeMap
        | GetFieldSlot | SetFieldSlot | MakeCompSlot | QueryFilter | QueryProject | MakeTuple
        | Unpack | GetIter | VecAdd | VecSub | VecMul | VecDiv | VecMod | VecNeg | VecNot
        | VecEq | VecNeq | VecLt | VecGt | VecLte | VecGte | VecFilter | VecSelect | LoadColumn
        | VecBroadcast | PopCheckErr | LogicalLoad | MaterializeAoS | ConcatN | BitAnd | BitOr
        | BitXor | ListGetLocal | ListGetLL | EqJF | NeqJF | LtJF | LteJF | GtJF | GteJF
        | EqConst | NeqConst | EqConstJF | NeqConstJF | ConstArith | Shl | Shr | BitNot => {
            OpcodeEffect::Pure
        }
    }
}

pub(crate) fn opcode_policy(boundary: EffectBoundary, op: Op) -> OpcodePolicy {
    use EffectBoundary::*;
    use OpcodeEffect::*;

    let effect = opcode_effect(op);
    match (boundary, effect) {
        (_, Pure | LocalMutation | DynamicCall) => OpcodePolicy::Allow,

        (Settlement, FrameExit | SettlementKernel | StateTransition) => OpcodePolicy::Allow,
        (Settlement, CapturedStateMutation) => OpcodePolicy::Reject(
            BoundaryViolation::SettlementEffect("captured-state mutation"),
        ),
        (Settlement, GlobalStateMutation) => {
            OpcodePolicy::Reject(BoundaryViolation::SettlementEffect("global-state mutation"))
        }
        (Settlement, AsyncTaskState) => {
            OpcodePolicy::Reject(BoundaryViolation::SettlementEffect("async/task state"))
        }
        (Settlement, TransactionalWorldMutation | DeclarationStateMutation) => {
            OpcodePolicy::Reject(BoundaryViolation::SettlementEffect("direct world mutation"))
        }
        (Settlement, EventEmission) => {
            OpcodePolicy::Reject(BoundaryViolation::SettlementEffect("event emission"))
        }
        (Settlement, SystemScheduling) => {
            OpcodePolicy::Reject(BoundaryViolation::SettlementEffect("system scheduling"))
        }
        (Settlement, Output) => OpcodePolicy::Reject(BoundaryViolation::SettlementEffect("output")),
        (Settlement, Snapshot | Rollback) => {
            OpcodePolicy::Reject(BoundaryViolation::SettlementEffect("timeline mutation"))
        }
        (Settlement, HandlerStateMutation) => OpcodePolicy::Reject(
            BoundaryViolation::SettlementEffect("handler state mutation"),
        ),
        (Settlement, VmTermination) => {
            OpcodePolicy::Reject(BoundaryViolation::SettlementEffect("VM termination"))
        }
        (Settlement, InteriorMutation) => OpcodePolicy::Reject(
            BoundaryViolation::SettlementEffect("interior heap mutation"),
        ),
        (Settlement, BeginTransaction | TransactionProgress | EndPostCommit) => {
            OpcodePolicy::Reject(BoundaryViolation::SettlementEffect(
                "ordinary transaction control",
            ))
        }

        (Transaction, TransactionalWorldMutation | TransactionProgress) => OpcodePolicy::Allow,
        (Transaction, FrameExit) => OpcodePolicy::CheckOwnerFrameExit,
        (Transaction, BeginTransaction) => {
            OpcodePolicy::Reject(BoundaryViolation::NestedTransaction)
        }
        (Transaction, SettlementKernel) => {
            OpcodePolicy::Reject(BoundaryViolation::SettlementKernel)
        }
        (Transaction, CapturedStateMutation | GlobalStateMutation) => {
            OpcodePolicy::Reject(BoundaryViolation::GlobalOrCapturedState)
        }
        (Transaction, AsyncTaskState) => OpcodePolicy::Reject(BoundaryViolation::AsyncBeforeCommit),
        (Transaction, EventEmission | StateTransition) => {
            OpcodePolicy::Reject(BoundaryViolation::EventBeforeCommit)
        }
        (Transaction, SystemScheduling) => {
            OpcodePolicy::Reject(BoundaryViolation::ScheduleBeforeCommit)
        }
        (Transaction, Output) => OpcodePolicy::Reject(BoundaryViolation::OutputBeforeCommit),
        (Transaction, Snapshot | Rollback | DeclarationStateMutation) => {
            OpcodePolicy::Reject(BoundaryViolation::TimelineOrDeclarationState)
        }
        (Transaction, InteriorMutation) => {
            OpcodePolicy::Reject(BoundaryViolation::InteriorMutation)
        }
        (Transaction, HandlerStateMutation | VmTermination | EndPostCommit) => {
            OpcodePolicy::Reject(BoundaryViolation::VmLifecycleState)
        }

        (PostCommit, FrameExit | VmTermination) => OpcodePolicy::CheckOwnerFrameExit,
        (PostCommit, EventEmission | AsyncTaskState | Output | Snapshot | InteriorMutation) => {
            OpcodePolicy::Allow
        }
        (PostCommit, HandlerStateMutation | EndPostCommit) => OpcodePolicy::Allow,
        (
            PostCommit,
            TransactionalWorldMutation
            | DeclarationStateMutation
            | CapturedStateMutation
            | GlobalStateMutation
            | Rollback
            | StateTransition
            | SystemScheduling
            | SettlementKernel
            | BeginTransaction
            | TransactionProgress,
        ) => OpcodePolicy::Reject(BoundaryViolation::AuthoritativeStateMutation),
    }
}

pub(crate) fn forbidden_builtin_effect(
    boundary: EffectBoundary,
    builtin: Builtin,
) -> Option<String> {
    match boundary {
        EffectBoundary::Settlement => {
            // These ECS effects append only to the active resolver's isolated
            // candidate patch. Settlement commit remains their sole adoption
            // path into the live world.
            if matches!(
                builtin,
                Builtin::InsertFact | Builtin::RemoveFact | Builtin::ReplaceFactBy
            ) {
                return None;
            }
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
            (!effects.is_pure() && !effects.is_readonly()).then(|| format!("{} effect", effects))
        }
        EffectBoundary::Transaction => {
            if matches!(
                builtin,
                Builtin::Set
                    | Builtin::SetResource
                    | Builtin::Spawn
                    | Builtin::Remove
                    | Builtin::Despawn
            ) {
                return None;
            }
            let effects = crate::builtins::builtin_effect(builtin.name());
            (!effects.is_pure() && !effects.is_readonly()).then(|| effects.to_string())
        }
        EffectBoundary::PostCommit => {
            let effects = crate::builtins::builtin_effect(builtin.name());
            effects
                .allows(crate::types::Effect::ECS)
                .then(|| effects.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_boundary_consumes_the_same_opcode_classification() {
        assert_eq!(
            opcode_policy(EffectBoundary::Settlement, Op::EcsSet),
            OpcodePolicy::Reject(BoundaryViolation::SettlementEffect("direct world mutation"))
        );
        assert_eq!(
            opcode_policy(EffectBoundary::Transaction, Op::EcsSet),
            OpcodePolicy::Allow
        );
        assert_eq!(
            opcode_policy(EffectBoundary::PostCommit, Op::EcsSet),
            OpcodePolicy::Reject(BoundaryViolation::AuthoritativeStateMutation)
        );
    }

    #[test]
    fn transaction_and_post_commit_control_have_distinct_owner_boundaries() {
        assert_eq!(
            opcode_policy(EffectBoundary::Transaction, Op::BeginTransaction),
            OpcodePolicy::Reject(BoundaryViolation::NestedTransaction)
        );
        assert_eq!(
            opcode_policy(EffectBoundary::Transaction, Op::Return),
            OpcodePolicy::CheckOwnerFrameExit
        );
        assert_eq!(
            opcode_policy(EffectBoundary::PostCommit, Op::EndPostCommit),
            OpcodePolicy::Allow
        );
    }

    #[test]
    fn builtin_profiles_share_one_effect_owner() {
        assert!(forbidden_builtin_effect(EffectBoundary::Transaction, Builtin::Set).is_none());
        assert!(forbidden_builtin_effect(EffectBoundary::PostCommit, Builtin::Set).is_some());
        assert!(
            forbidden_builtin_effect(EffectBoundary::Settlement, Builtin::InsertFact).is_none()
        );
    }
}
