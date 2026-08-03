use num_enum::TryFromPrimitive;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ConstKey {
    Nil,
    Bool(bool),
    Int(i64),
    Float(u64),
    Str(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(u8)]
pub enum Op {
    Const,
    Pop,
    Dup,

    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,

    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,

    Not,
    // Reserved/deprecated opcodes kept for bytecode compatibility.
    And, // do not emit: logical and compiles to short-circuit jumps
    Or,  // do not emit: logical or compiles to short-circuit jumps

    DefGlobal,
    GetGlobal,
    SetGlobal,
    GetLocal,
    SetLocal,
    MoveLocal,

    Jump,
    JumpIfFalse,
    JumpBack,

    Call,
    AsyncCall,
    Await,
    Yield,
    Return,
    Try,

    MakeList,
    MakeComp,
    GetField,
    SetField,
    GetIndex,
    SetIndex,

    EcsGet,
    EcsSet,
    EcsHas,
    EcsSpawn,
    EcsQuery,
    InitResource,

    MakeState,
    Transition,
    MakeVariant,

    Emit,
    RunSystem,
    RunSchedule,
    MatchState,
    IsVariant,
    Pipe, // reserved/deprecated, do not emit: pipe compiles to Call directly

    Print,
    Len,
    TypeOf,

    Break, // reserved/deprecated, do not emit: break compiles to Jump directly

    Closure,
    GetUpvalue,
    SetUpvalue,
    MakeMap,

    GetFieldSlot,
    SetFieldSlot,
    MakeCompSlot,

    QueryFilter,
    QueryProject,
    MakeTuple,
    Snapshot,
    Rollback,

    Unpack,

    GetIter,
    IterNext,

    ListPushLocal,

    BitsetSetInplace,
    BitsetClearInplace,
    BufferAppendInplace,
    ByteBufSetU8Inplace,
    ByteBufSetU32LeInplace,
    ByteBufSetI32LeInplace,

    VecAdd,
    VecSub,
    VecMul,
    VecDiv,
    VecMod,
    VecNeg,
    VecNot,
    VecEq,
    VecNeq,
    VecLt,
    VecGt,
    VecLte,
    VecGte,
    VecFilter,
    VecSelect,
    LoadColumn,

    /// Duplicate `fill` to match `template`'s list length (vectorized `map` with constant body).
    VecBroadcast,

    OnceGuardPass,

    PopCheckErr,

    Halt,

    /// Logical ECS/component load used by the layout-aware IR.
    LogicalLoad,
    /// Logical ECS/component store used by the layout-aware IR.
    LogicalStore,
    /// Explicit AoS materialization marker inserted at layout boundaries.
    MaterializeAoS,

    /// N-ary string concatenation (operand: u8 count). Pops `n` values and
    /// pushes one string built in a single exact-capacity buffer — an
    /// f-string with k parts costs one allocation instead of k-1 chained
    /// `Add`s, each of which re-copied the growing prefix (Tier-1 #2: the
    /// O(parts²) hiding under every f-string).
    ConcatN,

    /// Bitwise int ops (`&`, `|`, `^`) — added for bitboard workloads
    /// (sudoku/chess-style candidate masks). Int-only; appended at the end
    /// of the enum to preserve bytecode compatibility.
    BitAnd,
    BitOr,
    BitXor,

    /// `xs[i] = v` for a `let unique` local list (operand: u16 slot; stack:
    /// index, value). Mutates the list in the local slot directly instead of
    /// round-tripping it through the stack — the round trip held a second
    /// Arc reference, so EVERY indexed write cloned the whole list
    /// (profile-copies caught it in the sudoku solver's undo path: three
    /// 9-element clones per backtrack).
    ListSetLocal,

    /// `local[idx]` read fused into one dispatch (operand: u16 slot; stack:
    /// index). Same semantics as GetLocal+GetIndex for any indexable value
    /// in the slot — half the dispatch cost of the hot path in array code.
    ListGetLocal,

    /// Counted-range loop back-edge fused into one dispatch (operands:
    /// cur_slot u16, end_slot u16, back_delta u16). Increments the int in
    /// `cur_slot`; while `cur < end`, jumps back `back_delta` bytes (from
    /// the ip after the operands). Replaces GetLocal+Const+Add+SetLocal+
    /// JumpBack+GetLocal+GetLocal+Lt+JumpIfFalse — nine dispatches per
    /// iteration of every `for i in range(...)` loop. Charges fuel on the
    /// taken back-edge exactly like JumpBack.
    ForRangeNext,

    /// Pop `n` values in one dispatch (operand: u8 count). Scope exits used
    /// to emit one Pop per local — a per-iteration tax on every loop body
    /// with bindings (12% of all dispatches in the sudoku workload).
    PopN,

    /// `local_list[local_idx]` in one dispatch (operands: list_slot u16,
    /// idx_slot u16). The most common indexing shape in array code —
    /// GetLocal + ListGetLocal collapsed.
    ListGetLL,

    /// Two consecutive GetLocals in one dispatch (operands: slot1 u16,
    /// slot2 u16) — the operand-feeding shape of every binary op on locals.
    GetLocal2,

    /// Compare-and-branch superinstructions (operand: u16 absolute target,
    /// patched like JumpIfFalse). Pop b, pop a, jump when the comparison is
    /// FALSE — exactly `Cmp` + `JumpIfFalse`, one dispatch instead of two
    /// and no intermediate bool. Emitted by a peephole that never fuses
    /// across a label (see `Compiler::mark_label`).
    EqJF,
    NeqJF,
    LtJF,
    LteJF,
    GtJF,
    GteJF,

    /// `a == K` / `a != K` with a constant-pool rhs (operand: u16 const
    /// index). Pops one, pushes bool — `Const` + `Eq` in one dispatch.
    EqConst,
    NeqConst,
    /// Branch-fused forms (operands: u16 const index, u16 target).
    EqConstJF,
    NeqConstJF,
    /// `a OP K` (operands: u16 const index, u8 op byte of the underlying
    /// arithmetic/bitwise Op). Pops one, pushes one.
    ConstArith,
    /// `x = x + K` for an int local (operands: u16 slot, u16 const index).
    /// GetLocal+Const+Add+SetLocal in one dispatch — the shape of every
    /// counter in hot code.
    IncLocal,

    /// Int shifts (`<<`, `>>` expressions). Logical shifts on the u64 bit
    /// pattern; a count outside 0..64 yields 0 — identical semantics to the
    /// shl()/shr() builtins so operator and builtin can never disagree.
    Shl,
    Shr,

    /// Unary `~` — flip all 64 bits of an int (the revoke-mask idiom:
    /// `allowed & ~revokes`).
    BitNot,

    /// `emit E { .. } after N` — pop the event, pop the int delay, queue
    /// the event to fire after N event-flush cycles (game ticks). Timers
    /// live in the event queue, not in hand-rolled countdown fields.
    EmitAfter,

    /// `schedule serial [ ... ]` — same operand layout as `RunSchedule`
    /// (u16 count, then count× u16 system-ref constant indices), but every
    /// system runs one at a time in topological order: no worker snapshots,
    /// no merge (dogfood feature seq 83, the per-call `--serial-schedule`).
    RunScheduleSerial,

    /// RFC-0001 settlement transaction boundary and causal operations.
    BeginSettlement,
    EndSettlement,
    ProposeIntent,
    StageCandidate,
}

impl Op {
    pub fn from_byte(b: u8) -> Result<Self, String> {
        Self::try_from(b).map_err(|_| format!("Invalid opcode byte: {}", b))
    }
}

/// Mutable bytecode construction artifact. A builder never carries an
/// executable verification certificate; cloning or editing it therefore
/// cannot launder an earlier proof onto new bytes.
#[derive(Clone, Debug)]
pub struct ChunkBuilder {
    pub(crate) code: Vec<u8>,
    pub(crate) constants: Vec<crate::value::Value>,
    pub(crate) lines: Vec<u32>,
    pub(crate) name: String,
    dedup: HashMap<ConstKey, u16>,
    str_dedup: HashMap<String, u32>,
    next_str_id: u32,
}

/// Backward-compatible source name for the mutable construction artifact.
/// VM storage and execution use [`SealedChunk`], never `Chunk`.
pub type Chunk = ChunkBuilder;

/// Immutable executable bytecode and its inseparable structural proof.
#[derive(Clone, Debug)]
pub struct SealedChunk {
    inner: std::sync::Arc<ChunkBuilder>,
    proof: std::sync::Arc<crate::bytecode_verifier::VerifiedChunk>,
}

impl std::ops::Deref for SealedChunk {
    type Target = ChunkBuilder;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl SealedChunk {
    /// Return a mutable construction copy. The verification proof is
    /// deliberately discarded, so loading the result always reverifies it.
    pub fn to_builder(&self) -> ChunkBuilder {
        (*self.inner).clone()
    }

    pub fn instruction_count(&self) -> usize {
        self.proof.instruction_count
    }

    #[cfg(test)]
    pub(crate) fn from_unchecked_for_test(chunk: ChunkBuilder) -> Self {
        Self {
            inner: std::sync::Arc::new(chunk),
            proof: std::sync::Arc::new(crate::bytecode_verifier::VerifiedChunk {
                instruction_count: 0,
            }),
        }
    }
}

impl ChunkBuilder {
    pub fn new(name: &str) -> Self {
        ChunkBuilder {
            code: Vec::new(),
            constants: Vec::new(),
            lines: Vec::new(),
            name: name.to_string(),
            dedup: HashMap::new(),
            str_dedup: HashMap::new(),
            next_str_id: 0,
        }
    }

    pub fn write(&mut self, byte: u8, line: u32) {
        self.code.push(byte);
        self.lines.push(line);
    }

    pub fn write_op(&mut self, op: Op, line: u32) {
        self.write(op as u8, line);
    }

    fn make_key(&mut self, value: &crate::value::Value) -> Option<ConstKey> {
        if value.is_nil() {
            return Some(ConstKey::Nil);
        }
        if let Some(b) = value.as_bool() {
            return Some(ConstKey::Bool(b));
        }
        if let Some(i) = value.as_int() {
            return Some(ConstKey::Int(i));
        }
        if let Some(f) = value.as_float() {
            return Some(ConstKey::Float(f.to_bits()));
        }
        if let Some(s) = value.as_str() {
            let id = if let Some(&id) = self.str_dedup.get(s) {
                id
            } else {
                let id = self.next_str_id;
                self.next_str_id += 1;
                self.str_dedup.insert(s.to_owned(), id);
                id
            };
            return Some(ConstKey::Str(id));
        }
        None
    }

    pub fn add_constant(&mut self, value: crate::value::Value) -> u16 {
        if let Some(key) = self.make_key(&value) {
            if let Some(&idx) = self.dedup.get(&key) {
                return idx;
            }
            let idx = self.push_constant(value);
            self.dedup.insert(key, idx);
            return idx;
        }
        self.push_constant(value)
    }

    fn push_constant(&mut self, value: crate::value::Value) -> u16 {
        assert!(
            self.constants.len() <= u16::MAX as usize,
            "Constant pool overflow: chunk '{}' already contains {} constants (max {})",
            self.name,
            self.constants.len(),
            u16::MAX as usize + 1
        );
        self.constants.push(value);
        (self.constants.len() - 1) as u16
    }

    pub fn write_u16(&mut self, val: u16, line: u32) {
        self.write((val >> 8) as u8, line);
        self.write((val & 0xff) as u8, line);
    }

    pub fn read_u16(&self, offset: usize) -> u16 {
        ((self.code[offset] as u16) << 8) | (self.code[offset + 1] as u16)
    }

    pub fn write_const(&mut self, value: crate::value::Value, line: u32) {
        let idx = self.add_constant(value);
        self.write_op(Op::Const, line);
        self.write_u16(idx, line);
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }

    pub fn constants(&self) -> &[crate::value::Value] {
        &self.constants
    }

    pub fn lines(&self) -> &[u32] {
        &self.lines
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn verify_and_seal(
        self,
    ) -> Result<SealedChunk, crate::bytecode_verifier::VerificationError> {
        let proof = crate::bytecode_verifier::verify_chunk(&self)?;
        Ok(SealedChunk {
            inner: std::sync::Arc::new(self),
            proof: std::sync::Arc::new(proof),
        })
    }
}
