use std::collections::BTreeSet;

use super::{
    EntityOperand, OperationMetadata, PendingComponentWrite, PendingDespawn, PendingFactKey,
    PendingRelationOperation, PendingRelationValue, PendingSpawn, RelationRuntimeError,
    RelationRuntimeResult, RelationTransaction,
};

/// Versioned admission profile for host-created authoritative transactions.
///
/// The limits bound the transaction envelope before the candidate world is
/// cloned or mutated. Component payload values remain subject to the VM's
/// existing value/heap limits; their retained slots and schema text are
/// included in this profile's deterministic structural cost.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationTransactionProfile {
    pub max_spawns: usize,
    pub max_component_writes: usize,
    pub max_operations: usize,
    pub max_despawns: usize,
    pub max_values: usize,
    pub max_text_bytes: usize,
    pub max_metadata_entries: usize,
    pub max_candidate_handles: usize,
    pub max_structural_bytes: usize,
}

impl Default for RelationTransactionProfile {
    fn default() -> Self {
        Self {
            max_spawns: 16_384,
            max_component_writes: 65_536,
            max_operations: 65_536,
            max_despawns: 16_384,
            max_values: 1_048_576,
            max_text_bytes: 16 * 1024 * 1024,
            max_metadata_entries: 262_144,
            max_candidate_handles: 16_384,
            max_structural_bytes: 64 * 1024 * 1024,
        }
    }
}

/// An admitted transaction whose complete envelope fits one sealed profile.
///
/// Construction is the only public route to the inner transaction. Runtime
/// mutation APIs consume this type so oversized host input fails before a
/// candidate snapshot or allocator state is created.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedRelationTransaction {
    transaction: RelationTransaction,
}

impl BoundedRelationTransaction {
    pub(crate) fn try_new(
        transaction: RelationTransaction,
        profile: &RelationTransactionProfile,
    ) -> RelationRuntimeResult<Self> {
        TransactionMeter::admit(&transaction, profile)?;
        Ok(Self { transaction })
    }

    pub fn transaction(&self) -> &RelationTransaction {
        &self.transaction
    }

    pub fn into_transaction(self) -> RelationTransaction {
        self.transaction
    }
}

/// Incremental host-facing constructor. Every item is charged against a
/// private meter before it is retained in the transaction vectors.
pub struct BoundedRelationTransactionBuilder {
    transaction: RelationTransaction,
    meter: TransactionMeter,
}

impl BoundedRelationTransactionBuilder {
    pub fn new(profile: RelationTransactionProfile) -> Self {
        Self {
            transaction: RelationTransaction::default(),
            meter: TransactionMeter::new(profile),
        }
    }

    pub fn push_spawn(&mut self, spawn: PendingSpawn) -> RelationRuntimeResult<()> {
        check_next_count(
            "relation.transaction_spawn_limit",
            self.transaction.spawns.len(),
            self.meter.profile.max_spawns,
        )?;
        let mut meter = self.meter.clone();
        meter.spawn(&spawn)?;
        self.transaction.spawns.push(spawn);
        self.meter = meter;
        Ok(())
    }

    pub fn push_component_write(
        &mut self,
        write: PendingComponentWrite,
    ) -> RelationRuntimeResult<()> {
        check_next_count(
            "relation.transaction_component_write_limit",
            self.transaction.component_writes.len(),
            self.meter.profile.max_component_writes,
        )?;
        let mut meter = self.meter.clone();
        meter.component_write(&write)?;
        self.transaction.component_writes.push(write);
        self.meter = meter;
        Ok(())
    }

    pub fn push_operation(
        &mut self,
        operation: PendingRelationOperation,
    ) -> RelationRuntimeResult<()> {
        check_next_count(
            "relation.transaction_operation_limit",
            self.transaction.operations.len(),
            self.meter.profile.max_operations,
        )?;
        let mut meter = self.meter.clone();
        meter.operation(&operation)?;
        self.transaction.operations.push(operation);
        self.meter = meter;
        Ok(())
    }

    pub fn push_despawn(&mut self, despawn: PendingDespawn) -> RelationRuntimeResult<()> {
        check_next_count(
            "relation.transaction_despawn_limit",
            self.transaction.despawns.len(),
            self.meter.profile.max_despawns,
        )?;
        let mut meter = self.meter.clone();
        meter.despawn(&despawn)?;
        self.transaction.despawns.push(despawn);
        self.meter = meter;
        Ok(())
    }

    pub fn finish(self) -> BoundedRelationTransaction {
        BoundedRelationTransaction {
            transaction: self.transaction,
        }
    }
}

#[derive(Clone)]
struct TransactionMeter {
    profile: RelationTransactionProfile,
    values: usize,
    text_bytes: usize,
    metadata_entries: usize,
    structural_bytes: usize,
    candidate_handles: BTreeSet<u32>,
}

impl TransactionMeter {
    fn new(profile: RelationTransactionProfile) -> Self {
        Self {
            profile,
            values: 0,
            text_bytes: 0,
            metadata_entries: 0,
            structural_bytes: 0,
            candidate_handles: BTreeSet::new(),
        }
    }

    fn admit(
        transaction: &RelationTransaction,
        profile: &RelationTransactionProfile,
    ) -> RelationRuntimeResult<()> {
        check_count(
            "relation.transaction_spawn_limit",
            transaction.spawns.len(),
            profile.max_spawns,
        )?;
        check_count(
            "relation.transaction_component_write_limit",
            transaction.component_writes.len(),
            profile.max_component_writes,
        )?;
        check_count(
            "relation.transaction_operation_limit",
            transaction.operations.len(),
            profile.max_operations,
        )?;
        check_count(
            "relation.transaction_despawn_limit",
            transaction.despawns.len(),
            profile.max_despawns,
        )?;

        let mut meter = Self::new(profile.clone());
        for spawn in &transaction.spawns {
            meter.spawn(spawn)?;
        }
        for write in &transaction.component_writes {
            meter.component_write(write)?;
        }
        for operation in &transaction.operations {
            meter.operation(operation)?;
        }
        for despawn in &transaction.despawns {
            meter.despawn(despawn)?;
        }
        Ok(())
    }

    fn spawn(&mut self, spawn: &PendingSpawn) -> RelationRuntimeResult<()> {
        self.candidate_handle(spawn.handle)?;
        self.charge_structural(std::mem::size_of_val(spawn))?;
        if let Some(name) = &spawn.name {
            self.text(name)?;
        }
        Ok(())
    }

    fn component_write(&mut self, write: &PendingComponentWrite) -> RelationRuntimeResult<()> {
        self.entity_operand(write.entity)?;
        self.text(&write.component.type_name)?;
        for field in write.component.layout.iter() {
            self.text(field)?;
        }
        self.charge_values(write.component.values.len())?;
        self.charge_structural(
            write
                .component
                .values
                .len()
                .saturating_mul(std::mem::size_of::<crate::value::Value>()),
        )
    }

    fn despawn(&mut self, despawn: &PendingDespawn) -> RelationRuntimeResult<()> {
        self.charge_structural(std::mem::size_of_val(&despawn.entity))?;
        self.metadata(&despawn.metadata)
    }

    fn operation(&mut self, operation: &PendingRelationOperation) -> RelationRuntimeResult<()> {
        match operation {
            PendingRelationOperation::Insert { fact, metadata }
            | PendingRelationOperation::Remove { fact, metadata } => {
                self.fact(fact)?;
                self.metadata(metadata)
            }
            PendingRelationOperation::ReplaceBy {
                relation,
                unique_constraint,
                selected_key,
                tuple,
                metadata,
            } => {
                self.text(relation)?;
                self.text(unique_constraint)?;
                self.values(selected_key)?;
                self.values(tuple)?;
                self.metadata(metadata)
            }
        }
    }

    fn fact(&mut self, fact: &PendingFactKey) -> RelationRuntimeResult<()> {
        self.text(&fact.relation)?;
        self.values(&fact.tuple)
    }

    fn values(&mut self, values: &[PendingRelationValue]) -> RelationRuntimeResult<()> {
        self.charge_values(values.len())?;
        for value in values {
            self.charge_structural(std::mem::size_of_val(value))?;
            match value {
                PendingRelationValue::Entity(entity) => self.entity_operand(*entity)?,
                PendingRelationValue::Text(text) => self.text(text)?,
                PendingRelationValue::Int(_) | PendingRelationValue::Count(_) => {}
            }
        }
        Ok(())
    }

    fn entity_operand(&mut self, entity: EntityOperand) -> RelationRuntimeResult<()> {
        self.charge_structural(std::mem::size_of_val(&entity))?;
        if let EntityOperand::Candidate(handle) = entity {
            self.candidate_handle(handle)?;
        }
        Ok(())
    }

    fn candidate_handle(&mut self, handle: u32) -> RelationRuntimeResult<()> {
        self.candidate_handles.insert(handle);
        check_count(
            "relation.transaction_candidate_handle_limit",
            self.candidate_handles.len(),
            self.profile.max_candidate_handles,
        )
    }

    fn metadata(&mut self, metadata: &OperationMetadata) -> RelationRuntimeResult<()> {
        let entries = metadata
            .causes
            .len()
            .checked_add(metadata.required_capabilities.len())
            .ok_or_else(|| limit_error("relation.transaction_metadata_limit"))?;
        self.metadata_entries = self
            .metadata_entries
            .checked_add(entries)
            .ok_or_else(|| limit_error("relation.transaction_metadata_limit"))?;
        check_count(
            "relation.transaction_metadata_limit",
            self.metadata_entries,
            self.profile.max_metadata_entries,
        )?;
        for value in metadata
            .causes
            .iter()
            .chain(metadata.required_capabilities.iter())
        {
            self.text(value)?;
        }
        Ok(())
    }

    fn charge_values(&mut self, count: usize) -> RelationRuntimeResult<()> {
        self.values = self
            .values
            .checked_add(count)
            .ok_or_else(|| limit_error("relation.transaction_value_limit"))?;
        check_count(
            "relation.transaction_value_limit",
            self.values,
            self.profile.max_values,
        )
    }

    fn text(&mut self, value: &str) -> RelationRuntimeResult<()> {
        self.text_bytes = self
            .text_bytes
            .checked_add(value.len())
            .ok_or_else(|| limit_error("relation.transaction_text_limit"))?;
        check_count(
            "relation.transaction_text_limit",
            self.text_bytes,
            self.profile.max_text_bytes,
        )?;
        self.charge_structural(value.len())
    }

    fn charge_structural(&mut self, bytes: usize) -> RelationRuntimeResult<()> {
        self.structural_bytes = self
            .structural_bytes
            .checked_add(bytes)
            .ok_or_else(|| limit_error("relation.transaction_structural_limit"))?;
        check_count(
            "relation.transaction_structural_limit",
            self.structural_bytes,
            self.profile.max_structural_bytes,
        )
    }
}

fn check_next_count(code: &'static str, current: usize, limit: usize) -> RelationRuntimeResult<()> {
    let next = current.checked_add(1).ok_or_else(|| limit_error(code))?;
    check_count(code, next, limit)
}

fn check_count(code: &'static str, actual: usize, limit: usize) -> RelationRuntimeResult<()> {
    if actual <= limit {
        Ok(())
    } else {
        Err(RelationRuntimeError::new(
            code,
            format!("{actual} exceeds {limit}"),
        ))
    }
}

fn limit_error(code: &'static str) -> RelationRuntimeError {
    RelationRuntimeError::new(code, "counter overflow")
}
