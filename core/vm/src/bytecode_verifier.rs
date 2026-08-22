//! Structural verification for executable RAD bytecode.
//!
//! The verifier is intentionally independent from the source checker. Host
//! supplied chunks, corrupted compiler output, and fuzzed bytecode all cross
//! this boundary before the VM can execute one instruction.

use crate::opcode::{Chunk, Op};
use std::collections::{HashMap, VecDeque};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationError {
    pub chunk: String,
    pub offset: usize,
    pub message: String,
}

impl VerificationError {
    pub(crate) fn at(chunk: &Chunk, offset: usize, message: impl Into<String>) -> Self {
        Self {
            chunk: chunk.name.clone(),
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for VerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "bytecode verification failed in `{}` at byte {}: {}",
            self.chunk, self.offset, self.message
        )
    }
}

impl std::error::Error for VerificationError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AtomicState(Option<(AtomicRegionKind, usize)>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtomicRegionKind {
    Settlement,
    Transaction,
    PostCommit,
}

impl AtomicRegionKind {
    fn name(self) -> &'static str {
        match self {
            Self::Settlement => "settlement",
            Self::Transaction => "transaction",
            Self::PostCommit => "post_commit",
        }
    }
}

#[derive(Clone, Debug)]
struct Instruction {
    offset: usize,
    end: usize,
    op: Op,
    branches: Vec<usize>,
    falls_through: bool,
    may_return: bool,
}

/// Cached proof attached to an immutable loaded chunk.
#[derive(Clone, Debug)]
pub(crate) struct VerifiedChunk {
    pub(crate) instruction_count: usize,
}

fn read_u16(chunk: &Chunk, at: usize, instruction: usize) -> Result<u16, VerificationError> {
    let bytes = chunk.code.get(at..at + 2).ok_or_else(|| {
        VerificationError::at(chunk, instruction, "truncated u16 instruction operand")
    })?;
    Ok(((bytes[0] as u16) << 8) | bytes[1] as u16)
}

fn require_bytes(
    chunk: &Chunk,
    instruction: usize,
    operand_start: usize,
    count: usize,
) -> Result<usize, VerificationError> {
    let end = operand_start.checked_add(count).ok_or_else(|| {
        VerificationError::at(chunk, instruction, "instruction operand length overflow")
    })?;
    if end > chunk.code.len() {
        return Err(VerificationError::at(
            chunk,
            instruction,
            format!(
                "truncated {:?} instruction",
                Op::from_byte(chunk.code[instruction]).ok()
            ),
        ));
    }
    Ok(end)
}

fn fixed_operand_bytes(op: Op) -> Option<usize> {
    use Op::*;
    match op {
        Closure | RunSchedule | RunScheduleSerial | BeginTransaction => None,

        Const
        | DefGlobal
        | GetGlobal
        | SetGlobal
        | GetLocal
        | SetLocal
        | MoveLocal
        | GetUpvalue
        | SetUpvalue
        | Jump
        | JumpIfFalse
        | JumpBack
        | MakeList
        | MakeTuple
        | MakeMap
        | GetField
        | SetField
        | EcsGet
        | EcsHas
        | RunSystem
        | Transition
        | GetFieldSlot
        | SetFieldSlot
        | ListPushLocal
        | ListSetLocal
        | ListGetLocal
        | IsVariant
        | EqJF
        | NeqJF
        | LtJF
        | LteJF
        | GtJF
        | GteJF
        | EqConst
        | NeqConst
        | LogicalLoad
        | ProposeIntent
        | ReadBaseComponent
        | ReadCandidateComponent
        | RequireConstraint
        | EmitPhase
        | RunViewKernel => Some(2),

        CheckTransaction => Some(3),

        GetLocal2 | EqConstJF | NeqConstJF | IncLocal | ListGetLL | MakeComp | MakeCompSlot
        | InitResource | MakeState | MatchState | EcsReadField | EcsWriteField => Some(4),

        ForRangeNext | MakeVariant => Some(6),

        ConstArith | LoadColumn => Some(3),

        PopN | Call | AsyncCall | ConcatN | Print | IterNext | QueryFilter | QueryProject
        | EndTransaction => Some(1),

        EcsSpawn => Some(4),
        EcsQuery => Some(2),

        Pop
        | Dup
        | Add
        | Sub
        | Mul
        | Div
        | Mod
        | Neg
        | Eq
        | Neq
        | Lt
        | Gt
        | Lte
        | Gte
        | Not
        | Await
        | Yield
        | Return
        | Try
        | GetIndex
        | SetIndex
        | EcsSet
        | Emit
        | TypeOf
        | Len
        | Snapshot
        | Rollback
        | Unpack
        | GetIter
        | BitsetSetInplace
        | BitsetClearInplace
        | BufferAppendInplace
        | ByteBufSetU8Inplace
        | ByteBufSetU32LeInplace
        | ByteBufSetI32LeInplace
        | VecAdd
        | VecSub
        | VecMul
        | VecDiv
        | VecMod
        | VecNeg
        | VecNot
        | VecEq
        | VecNeq
        | VecLt
        | VecGt
        | VecLte
        | VecGte
        | VecFilter
        | VecSelect
        | VecBroadcast
        | OnceGuardPass
        | PopCheckErr
        | Halt
        | LogicalStore
        | MaterializeAoS
        | BitAnd
        | BitOr
        | BitXor
        | Shl
        | Shr
        | BitNot
        | EmitAfter
        | EmitSync
        | BeginSettlement
        | EndSettlement
        | StageCandidate
        | EndPostCommit => Some(0),
    }
}

fn decode_instruction(chunk: &Chunk, offset: usize) -> Result<Instruction, VerificationError> {
    let byte = *chunk
        .code
        .get(offset)
        .ok_or_else(|| VerificationError::at(chunk, offset, "missing opcode"))?;
    let op =
        Op::from_byte(byte).map_err(|message| VerificationError::at(chunk, offset, message))?;
    let operands = offset + 1;
    let end = if let Some(width) = fixed_operand_bytes(op) {
        require_bytes(chunk, offset, operands, width)?
    } else {
        match op {
            Op::Closure => {
                let header_end = require_bytes(chunk, offset, operands, 4)?;
                let captures = chunk.code[operands + 3] as usize;
                let capture_bytes = captures.checked_mul(3).ok_or_else(|| {
                    VerificationError::at(chunk, offset, "closure capture length overflow")
                })?;
                require_bytes(chunk, offset, header_end, capture_bytes)?
            }
            Op::RunSchedule | Op::RunScheduleSerial => {
                let count = read_u16(chunk, operands, offset)? as usize;
                let entries = count.checked_mul(2).ok_or_else(|| {
                    VerificationError::at(chunk, offset, "schedule operand length overflow")
                })?;
                require_bytes(chunk, offset, operands + 2, entries)?
            }
            Op::BeginTransaction => {
                let header_end = require_bytes(chunk, offset, operands, 4)?;
                let count = read_u16(chunk, operands + 2, offset)? as usize;
                let entries = count.checked_mul(2).ok_or_else(|| {
                    VerificationError::at(chunk, offset, "transaction operand length overflow")
                })?;
                require_bytes(chunk, offset, header_end, entries)?
            }
            _ => unreachable!("all variable-width opcodes are handled"),
        }
    };

    if op == Op::ConstArith {
        let embedded = chunk.code[operands + 2];
        let embedded = Op::from_byte(embedded)
            .map_err(|message| VerificationError::at(chunk, offset, message))?;
        if !matches!(
            embedded,
            Op::Add
                | Op::Sub
                | Op::Mul
                | Op::Div
                | Op::Mod
                | Op::BitAnd
                | Op::BitOr
                | Op::BitXor
                | Op::Shl
                | Op::Shr
        ) {
            return Err(VerificationError::at(
                chunk,
                offset,
                format!("invalid ConstArith embedded opcode {:?}", embedded),
            ));
        }
    }

    let absolute = |at: usize| read_u16(chunk, at, offset).map(|v| v as usize);
    let mut branches = Vec::new();
    let falls_through = match op {
        Op::Jump => {
            branches.push(absolute(operands)?);
            false
        }
        Op::JumpBack => {
            let delta = absolute(operands)?;
            let target = end.checked_sub(delta).ok_or_else(|| {
                VerificationError::at(chunk, offset, "JumpBack target underflows the chunk")
            })?;
            branches.push(target);
            false
        }
        Op::JumpIfFalse | Op::EqJF | Op::NeqJF | Op::LtJF | Op::LteJF | Op::GtJF | Op::GteJF => {
            branches.push(absolute(operands)?);
            true
        }
        Op::EqConstJF | Op::NeqConstJF | Op::MatchState => {
            branches.push(absolute(operands + 2)?);
            true
        }
        Op::ForRangeNext => {
            let delta = absolute(operands + 4)?;
            let target = end.checked_sub(delta).ok_or_else(|| {
                VerificationError::at(chunk, offset, "ForRangeNext target underflows the chunk")
            })?;
            branches.push(target);
            true
        }
        Op::Return | Op::Halt => false,
        _ => true,
    };

    Ok(Instruction {
        offset,
        end,
        op,
        branches,
        falls_through,
        may_return: matches!(op, Op::Return | Op::Halt | Op::Try),
    })
}

fn constant_indices(
    chunk: &Chunk,
    instruction: &Instruction,
) -> Result<Vec<usize>, VerificationError> {
    let at = instruction.offset + 1;
    let one = |offset| read_u16(chunk, offset, instruction.offset).map(|v| v as usize);
    let indices = match instruction.op {
        Op::Const
        | Op::EqConst
        | Op::NeqConst
        | Op::MakeComp
        | Op::GetField
        | Op::SetField
        | Op::EcsGet
        | Op::EcsHas
        | Op::InitResource
        | Op::Transition
        | Op::RunSystem
        | Op::IsVariant
        | Op::MakeCompSlot
        | Op::LoadColumn
        | Op::ProposeIntent
        | Op::ReadBaseComponent
        | Op::ReadCandidateComponent
        | Op::RequireConstraint => vec![one(at)?],
        Op::EcsReadField | Op::EcsWriteField => vec![one(at)?, one(at + 2)?],
        Op::CheckTransaction => vec![one(at + 1)?],
        Op::BeginTransaction => {
            let count = one(at + 2)?;
            let mut out = Vec::with_capacity(count + 1);
            out.push(one(at)?);
            for index in 0..count {
                out.push(one(at + 4 + index * 2)?);
            }
            out
        }
        Op::EqConstJF | Op::NeqConstJF | Op::ConstArith => vec![one(at)?],
        Op::IncLocal => vec![one(at + 2)?],
        Op::MakeState | Op::MakeVariant => vec![one(at)?, one(at + 2)?],
        Op::MatchState => vec![one(at)?],
        Op::EcsSpawn if chunk.code[at + 1] != 1 => vec![one(at + 2)?],
        Op::RunSchedule | Op::RunScheduleSerial => {
            let count = one(at)?;
            let mut out = Vec::with_capacity(count);
            for index in 0..count {
                out.push(one(at + 2 + index * 2)?);
            }
            out
        }
        _ => Vec::new(),
    };
    Ok(indices)
}

pub(crate) fn verify_chunk(chunk: &Chunk) -> Result<VerifiedChunk, VerificationError> {
    if chunk.code.len() != chunk.lines.len() {
        return Err(VerificationError::at(
            chunk,
            chunk.code.len().min(chunk.lines.len()),
            format!(
                "code/line table length mismatch ({} bytes, {} line entries)",
                chunk.code.len(),
                chunk.lines.len()
            ),
        ));
    }
    if chunk.code.is_empty() {
        return Err(VerificationError::at(chunk, 0, "empty chunk"));
    }

    let mut instructions = Vec::new();
    let mut boundaries = vec![false; chunk.code.len() + 1];
    let mut offset = 0usize;
    while offset < chunk.code.len() {
        boundaries[offset] = true;
        let instruction = decode_instruction(chunk, offset)?;
        offset = instruction.end;
        instructions.push(instruction);
    }
    boundaries[chunk.code.len()] = true;

    for instruction in &instructions {
        for index in constant_indices(chunk, instruction)? {
            if index >= chunk.constants.len() {
                return Err(VerificationError::at(
                    chunk,
                    instruction.offset,
                    format!(
                        "constant index {} is outside pool of length {}",
                        index,
                        chunk.constants.len()
                    ),
                ));
            }
        }
        match instruction.op {
            Op::BeginTransaction => {
                let count = read_u16(chunk, instruction.offset + 3, instruction.offset)? as usize;
                if count == 0 {
                    return Err(VerificationError::at(
                        chunk,
                        instruction.offset,
                        "BeginTransaction requires at least one changes_only authority",
                    ));
                }
                let name = read_u16(chunk, instruction.offset + 1, instruction.offset)? as usize;
                if chunk.constants[name].as_str().is_none() {
                    return Err(VerificationError::at(
                        chunk,
                        instruction.offset,
                        "BeginTransaction name constant must be a string",
                    ));
                }
                for authority in 0..count {
                    let index = read_u16(
                        chunk,
                        instruction.offset + 5 + authority * 2,
                        instruction.offset,
                    )? as usize;
                    if chunk.constants[index].as_str().is_none() {
                        return Err(VerificationError::at(
                            chunk,
                            instruction.offset,
                            "BeginTransaction changes_only constants must be strings",
                        ));
                    }
                }
            }
            Op::CheckTransaction => {
                let phase = chunk.code[instruction.offset + 1];
                if phase > 1 {
                    return Err(VerificationError::at(
                        chunk,
                        instruction.offset,
                        "CheckTransaction phase must be 0 (requires) or 1 (ensures)",
                    ));
                }
                let label = read_u16(chunk, instruction.offset + 2, instruction.offset)? as usize;
                if chunk.constants[label].as_str().is_none() {
                    return Err(VerificationError::at(
                        chunk,
                        instruction.offset,
                        "CheckTransaction label constant must be a string",
                    ));
                }
            }
            Op::EndTransaction if chunk.code[instruction.offset + 1] > 1 => {
                return Err(VerificationError::at(
                    chunk,
                    instruction.offset,
                    "EndTransaction post_commit flag must be 0 or 1",
                ));
            }
            _ => {}
        }
    }

    let mut lexical_state = HashMap::with_capacity(instructions.len());
    let mut active_region = None;
    for instruction in &instructions {
        lexical_state.insert(instruction.offset, AtomicState(active_region));
        match instruction.op {
            Op::BeginSettlement | Op::BeginTransaction => {
                if active_region.is_some() {
                    return Err(VerificationError::at(
                        chunk,
                        instruction.offset,
                        format!("nested {:?} atomic region", instruction.op),
                    ));
                }
                let kind = if instruction.op == Op::BeginSettlement {
                    AtomicRegionKind::Settlement
                } else {
                    AtomicRegionKind::Transaction
                };
                active_region = Some((kind, instruction.offset));
            }
            Op::EndSettlement | Op::EndTransaction | Op::EndPostCommit => {
                let expected = if instruction.op == Op::EndSettlement {
                    AtomicRegionKind::Settlement
                } else if instruction.op == Op::EndTransaction {
                    AtomicRegionKind::Transaction
                } else {
                    AtomicRegionKind::PostCommit
                };
                let Some((actual, _)) = active_region else {
                    return Err(VerificationError::at(
                        chunk,
                        instruction.offset,
                        format!("{:?} without a matching begin marker", instruction.op),
                    ));
                };
                if actual != expected {
                    return Err(VerificationError::at(
                        chunk,
                        instruction.offset,
                        format!(
                            "{:?} cannot close an active {}",
                            instruction.op,
                            actual.name()
                        ),
                    ));
                }
                active_region = if instruction.op == Op::EndTransaction
                    && chunk.code[instruction.offset + 1] == 1
                {
                    Some((AtomicRegionKind::PostCommit, instruction.offset))
                } else {
                    None
                };
            }
            Op::CheckTransaction
                if !matches!(active_region, Some((AtomicRegionKind::Transaction, _))) =>
            {
                return Err(VerificationError::at(
                    chunk,
                    instruction.offset,
                    "CheckTransaction is only valid inside a transaction",
                ));
            }
            _ if instruction.may_return && active_region.is_some() => {
                let kind = active_region.unwrap().0.name();
                return Err(VerificationError::at(
                    chunk,
                    instruction.offset,
                    format!("{:?} can leave an active {kind}", instruction.op),
                ));
            }
            _ => {}
        }
    }
    if let Some((kind, begin)) = active_region {
        return Err(VerificationError::at(
            chunk,
            begin,
            format!("active {} has no matching end marker", kind.name()),
        ));
    }

    for instruction in &instructions {
        let mut targets = instruction.branches.clone();
        if instruction.falls_through {
            if instruction.end == chunk.code.len() {
                return Err(VerificationError::at(
                    chunk,
                    instruction.offset,
                    "reachable control flow falls off the end of the chunk",
                ));
            }
            targets.push(instruction.end);
        }
        let source_after = match instruction.op {
            Op::BeginSettlement => {
                AtomicState(Some((AtomicRegionKind::Settlement, instruction.offset)))
            }
            Op::BeginTransaction => {
                AtomicState(Some((AtomicRegionKind::Transaction, instruction.offset)))
            }
            Op::EndSettlement | Op::EndPostCommit => AtomicState(None),
            Op::EndTransaction if chunk.code[instruction.offset + 1] == 1 => {
                AtomicState(Some((AtomicRegionKind::PostCommit, instruction.offset)))
            }
            Op::EndTransaction => AtomicState(None),
            _ => lexical_state[&instruction.offset],
        };
        for target in targets {
            if target >= chunk.code.len() || !boundaries[target] {
                return Err(VerificationError::at(
                    chunk,
                    instruction.offset,
                    format!(
                        "control-flow target {} is not an instruction boundary",
                        target
                    ),
                ));
            }
            let target_state = lexical_state[&target];
            if target_state != source_after {
                return Err(VerificationError::at(
                    chunk,
                    instruction.offset,
                    format!(
                        "control-flow edge to byte {} crosses {} boundary {:?} -> {:?}",
                        target,
                        source_after
                            .0
                            .or(target_state.0)
                            .map(|(kind, _)| kind.name())
                            .unwrap_or("atomic region"),
                        source_after.0,
                        target_state.0
                    ),
                ));
            }
        }
    }

    // A worklist proves that every reachable CFG join receives one exact
    // atomic-region token. The lexical edge check above also covers unreachable
    // malformed regions, so neither defense depends on reachability alone.
    let by_offset: HashMap<usize, &Instruction> =
        instructions.iter().map(|i| (i.offset, i)).collect();
    let mut incoming = HashMap::<usize, AtomicState>::new();
    let mut queue = VecDeque::from([(0usize, AtomicState(None))]);
    while let Some((at, state)) = queue.pop_front() {
        if let Some(previous) = incoming.insert(at, state) {
            if previous != state {
                return Err(VerificationError::at(
                    chunk,
                    at,
                    format!(
                        "CFG join has incompatible atomic states {:?} and {:?}",
                        previous.0, state.0
                    ),
                ));
            }
            continue;
        }
        let instruction = by_offset[&at];
        if lexical_state[&at] != state {
            return Err(VerificationError::at(
                chunk,
                at,
                "reachable instruction disagrees with its lexical atomic state",
            ));
        }
        let next_state = match instruction.op {
            Op::BeginSettlement => {
                AtomicState(Some((AtomicRegionKind::Settlement, instruction.offset)))
            }
            Op::BeginTransaction => {
                AtomicState(Some((AtomicRegionKind::Transaction, instruction.offset)))
            }
            Op::EndSettlement | Op::EndPostCommit => AtomicState(None),
            Op::EndTransaction if chunk.code[instruction.offset + 1] == 1 => {
                AtomicState(Some((AtomicRegionKind::PostCommit, instruction.offset)))
            }
            Op::EndTransaction => AtomicState(None),
            _ => state,
        };
        for target in &instruction.branches {
            queue.push_back((*target, next_state));
        }
        if instruction.falls_through {
            queue.push_back((instruction.end, next_state));
        }
    }

    Ok(VerifiedChunk {
        instruction_count: instructions.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(name: &str, bytes: &[(Op, &[u8])]) -> Chunk {
        let mut chunk = Chunk::new(name);
        for (op, operands) in bytes {
            chunk.write_op(*op, 1);
            for byte in *operands {
                chunk.write(*byte, 1);
            }
        }
        chunk
    }

    #[test]
    fn balanced_settlement_verifies() {
        let chunk = chunk(
            "balanced",
            &[
                (Op::BeginSettlement, &[]),
                (Op::EndSettlement, &[]),
                (Op::Const, &[0, 0]),
                (Op::Return, &[]),
            ],
        );
        let mut chunk = chunk;
        chunk.add_constant(crate::value::Value::NIL);
        assert!(verify_chunk(&chunk).is_ok());
    }

    fn transaction_chunk(has_post_commit: u8, close_post_commit: bool) -> Chunk {
        let mut bytes = vec![
            (Op::BeginTransaction, vec![0, 0, 0, 1, 0, 1]),
            (Op::Const, vec![0, 3]),
            (Op::CheckTransaction, vec![0, 0, 2]),
            (Op::Const, vec![0, 3]),
            (Op::CheckTransaction, vec![1, 0, 2]),
            (Op::EndTransaction, vec![has_post_commit]),
        ];
        if close_post_commit {
            bytes.push((Op::EndPostCommit, vec![]));
        }
        bytes.push((Op::Const, vec![0, 4]));
        bytes.push((Op::Return, vec![]));
        let borrowed = bytes
            .iter()
            .map(|(op, operands)| (*op, operands.as_slice()))
            .collect::<Vec<_>>();
        let mut chunk = chunk("transaction", &borrowed);
        chunk.add_constant(crate::value::Value::from_string(
            &mut crate::value::PersistentStore,
            "Rewrite".to_string(),
        ));
        chunk.add_constant(crate::value::Value::from_string(
            &mut crate::value::PersistentStore,
            "Marker".to_string(),
        ));
        chunk.add_constant(crate::value::Value::from_string(
            &mut crate::value::PersistentStore,
            "contract".to_string(),
        ));
        chunk.add_constant(crate::value::Value::from_bool(true));
        chunk.add_constant(crate::value::Value::NIL);
        chunk
    }

    #[test]
    fn transaction_and_post_commit_regions_verify_as_one_balanced_boundary() {
        assert!(verify_chunk(&transaction_chunk(0, false)).is_ok());
        assert!(verify_chunk(&transaction_chunk(1, true)).is_ok());
    }

    #[test]
    fn transaction_metadata_and_post_commit_balance_are_verified() {
        let missing_post_commit_end = transaction_chunk(1, false);
        assert!(verify_chunk(&missing_post_commit_end)
            .unwrap_err()
            .message
            .contains("active post_commit"));

        let mut invalid_flag = transaction_chunk(0, false);
        let end = invalid_flag
            .code
            .iter()
            .position(|byte| *byte == Op::EndTransaction as u8)
            .unwrap();
        invalid_flag.code[end + 1] = 2;
        assert!(verify_chunk(&invalid_flag)
            .unwrap_err()
            .message
            .contains("flag must be 0 or 1"));

        let unmatched = chunk(
            "unmatched-post-commit",
            &[(Op::EndPostCommit, &[]), (Op::Halt, &[])],
        );
        assert!(verify_chunk(&unmatched)
            .unwrap_err()
            .message
            .contains("without a matching begin marker"));
    }

    #[test]
    fn jump_cannot_leave_settlement_or_enter_an_operand() {
        let escaping = chunk(
            "escaping",
            &[
                (Op::BeginSettlement, &[]),
                (Op::Jump, &[0, 5]),
                (Op::EndSettlement, &[]),
                (Op::Halt, &[]),
            ],
        );
        assert!(verify_chunk(&escaping)
            .unwrap_err()
            .message
            .contains("crosses settlement"));

        let middle = chunk("middle", &[(Op::Jump, &[0, 2]), (Op::Halt, &[])]);
        assert!(verify_chunk(&middle)
            .unwrap_err()
            .message
            .contains("instruction boundary"));
    }

    #[test]
    fn malformed_encoding_and_settlement_markers_are_rejected() {
        let mut unknown = Chunk::new("unknown");
        unknown.write(u8::MAX, 1);
        assert!(verify_chunk(&unknown)
            .unwrap_err()
            .message
            .contains("Invalid opcode"));

        let truncated = chunk("truncated", &[(Op::Const, &[0])]);
        assert!(verify_chunk(&truncated)
            .unwrap_err()
            .message
            .contains("truncated"));

        let nested = chunk(
            "nested",
            &[
                (Op::BeginSettlement, &[]),
                (Op::BeginSettlement, &[]),
                (Op::EndSettlement, &[]),
                (Op::EndSettlement, &[]),
                (Op::Halt, &[]),
            ],
        );
        assert!(verify_chunk(&nested)
            .unwrap_err()
            .message
            .contains("nested BeginSettlement"));

        let unmatched = chunk("unmatched", &[(Op::EndSettlement, &[]), (Op::Halt, &[])]);
        assert!(verify_chunk(&unmatched)
            .unwrap_err()
            .message
            .contains("without a matching"));
    }

    #[test]
    fn conditional_join_cannot_enter_a_settlement_body() {
        // JumpIfFalse at 0 targets Pop at 4; fallthrough reaches the same Pop
        // through BeginSettlement at 3. The join would therefore mix Outside
        // and Inside(region 3).
        let mixed = chunk(
            "mixed-join",
            &[
                (Op::JumpIfFalse, &[0, 4]),
                (Op::BeginSettlement, &[]),
                (Op::Pop, &[]),
                (Op::EndSettlement, &[]),
                (Op::Halt, &[]),
            ],
        );
        assert!(verify_chunk(&mixed)
            .unwrap_err()
            .message
            .contains("crosses settlement"));
    }

    #[test]
    fn return_and_try_cannot_escape_an_active_region() {
        for op in [Op::Return, Op::Try, Op::Halt] {
            let escaping = chunk(
                &format!("escape-{op:?}"),
                &[(Op::BeginSettlement, &[]), (op, &[])],
            );
            assert!(verify_chunk(&escaping)
                .unwrap_err()
                .message
                .contains("active settlement"));
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        let mut seed = 0x7a11_c0de_5eed_u64;
        for len in 0..256usize {
            for _ in 0..64 {
                let mut chunk = Chunk::new("arbitrary");
                for _ in 0..len {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    chunk.write(seed as u8, 1);
                }
                let result = std::panic::catch_unwind(|| verify_chunk(&chunk));
                assert!(result.is_ok(), "verifier panicked for {} bytes", len);
            }
        }
    }
}
