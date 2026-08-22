// The fused `visit_view` kernel: one opcode that resolves its write
// addresses once, then walks a materialized view charging the semantic work
// each row performs.

#[derive(Clone, Copy)]
enum ResolvedViewKernelOperand {
    Field(crate::world::ResolvedFieldAddress),
    Int(i64),
    Float(f64),
}

#[derive(Clone, Copy)]
struct ResolvedViewKernelWrite {
    target: crate::world::ResolvedFieldAddress,
    left: ResolvedViewKernelOperand,
    right: ResolvedViewKernelOperand,
    refresh_view: bool,
}

impl VM {
    pub(crate) fn exec_view_kernel(&mut self) -> Result<(), String> {
        use crate::view_kernel::{ViewKernelArithmetic, ViewKernelOperand};

        let kernel_index = self.read_u16()? as usize;
        let plan = self
            .view_kernels
            .get(kernel_index)
            .cloned()
            .ok_or_else(|| format!("Invalid compiled view kernel {kernel_index}"))?;

        let dependencies = self
            .world
            .materialized_view_dependencies(&plan.view)
            .ok_or_else(|| format!("Unknown materialized view '{}'", plan.view))?;
        for dependency in dependencies {
            self.sandbox_check_read(dependency)?;
        }
        for write in &plan.writes {
            self.sandbox_check_write(&write.component)?;
            self.transaction_check_write(&write.component)?;
            for operand in [&write.left, &write.right] {
                if let ViewKernelOperand::Field { component, .. } = operand {
                    self.sandbox_check_read(component)?;
                }
            }
        }

        // An empty view has no rows to write, and resolving a write address
        // requires the target component to have live storage. Resolving first
        // turned "no entity has ever had this component" into a runtime error
        // for a system that simply ran before anything was spawned. The
        // capability checks above still run: an empty view must not become a
        // hole through which an ungranted write escapes review.
        let first_entity = self
            .world
            .materialized_view_first_entity(&plan.view)
            .ok_or_else(|| format!("Unknown materialized view '{}'", plan.view))?;
        let Some(first_entity) = first_entity else {
            self.push(Value::NIL);
            return Ok(());
        };

        let mut resolved = smallvec::SmallVec::<[ResolvedViewKernelWrite; 4]>::new();
        for write in &plan.writes {
            resolved.push(ResolvedViewKernelWrite {
                target: self.resolve_view_kernel_field(&write.component, &write.field)?,
                left: self.resolve_view_kernel_operand(&write.left)?,
                right: self.resolve_view_kernel_operand(&write.right)?,
                refresh_view: self
                    .world
                    .field_has_view_dependents(&write.component, &write.field),
            });
        }

        // What one row of this kernel costs, in the units the interpreter
        // charges ordinary bytecode. Fusing `visit_view` into a single opcode
        // must not make the traversal cheaper against `@budget(instructions:)`
        // than the equivalent unfused loop, so each row is charged for the work
        // it actually performs rather than a flat one unit per row.
        const ROW_VISIT_COST: u64 = 1;
        const OPERAND_READ_COST: u64 = 1;
        const ARITHMETIC_COST: u64 = 1;
        const FIELD_WRITE_COST: u64 = 1;
        const PROVENANCE_COST: u64 = 1;
        const VIEW_REFRESH_COST: u64 = 1;
        let row_cost = ROW_VISIT_COST
            + resolved
                .iter()
                .map(|write| {
                    2 * OPERAND_READ_COST
                        + ARITHMETIC_COST
                        + FIELD_WRITE_COST
                        + PROVENANCE_COST
                        + if write.refresh_view {
                            VIEW_REFRESH_COST
                        } else {
                            0
                        }
                })
                .sum::<u64>();
        let stable_revision = self
            .world
            .materialized_view_revision(&plan.view)
            .expect("view disappeared after dependency lookup");
        let mut current = Some(first_entity);
        while let Some(entity) = current {
            self.charge_work(row_cost)?;
            let next = self
                .world
                .materialized_view_next_entity(&plan.view, entity)
                .expect("view disappeared during kernel traversal");
            let entity_address = self
                .world
                .resolve_entity_address(entity)
                .ok_or_else(|| format!("view '{}' contains missing entity {entity}", plan.view))?;
            let entity_name = self.world.entity_name(entity);
            for (write, resolved) in plan.writes.iter().zip(&resolved) {
                let left = self.read_resolved_view_kernel_operand(entity, entity_address, resolved.left)?;
                let right =
                    self.read_resolved_view_kernel_operand(entity, entity_address, resolved.right)?;
                let value = match write.operation {
                    ViewKernelArithmetic::Add => helpers::binary_add(&mut self.gc, left, right)?,
                    ViewKernelArithmetic::Sub => helpers::binary_sub(&mut self.gc, left, right)?,
                    ViewKernelArithmetic::Mul => {
                        helpers::binary_mul(&mut self.gc, left, right, self.mem_limit)?
                    }
                    ViewKernelArithmetic::Div => helpers::binary_div(&mut self.gc, left, right)?,
                };
                self.transaction_journal_component(entity, &write.component);
                let persisted = value.deep_copy(&mut crate::value::PersistentStore);
                if self.is_worker {
                    let buffered = value.deep_copy(&mut crate::value::PersistentStore);
                    if !self.world.set_component_field_resolved_owned(
                        entity_address,
                        resolved.target,
                        persisted,
                    ) {
                        unsafe { buffered.release_persistent() };
                        return Err(format!(
                            "write_field() failed to update '{}.{}' on entity {entity}",
                            write.component, write.field
                        ));
                    }
                    self.command_buffer.push(crate::vm::EcsCommand::SetField(
                        entity,
                        write.component.clone(),
                        write.field.clone(),
                        buffered,
                    ));
                } else if !self.world.set_component_field_resolved_owned(
                    entity_address,
                    resolved.target,
                    persisted,
                ) {
                    return Err(format!(
                        "write_field() failed to update '{}.{}' on entity {entity}",
                        write.component, write.field
                    ));
                }
                self.record_causal_field_write_resolved(
                    entity,
                    entity_name.clone(),
                    &write.component,
                    &write.field,
                    value,
                    resolved.refresh_view,
                );
            }
            let revision = self
                .world
                .materialized_view_revision(&plan.view)
                .expect("view disappeared during kernel traversal");
            if revision != stable_revision {
                return Err(format!(
                    "visit_view({}) was invalidated by its callback: revision {} -> {}; stage membership-changing writes outside allocation-free traversal",
                    plan.view, stable_revision, revision
                ));
            }
            current = next;
        }
        self.push(Value::NIL);
        Ok(())
    }

    fn resolve_view_kernel_field(
        &self,
        component: &str,
        field: &str,
    ) -> Result<crate::world::ResolvedFieldAddress, String> {
        let layout = self
            .component_layouts
            .get(component)
            .ok_or_else(|| format!("Unknown component '{component}' in compiled view kernel"))?;
        let field_index = layout
            .iter()
            .position(|candidate| candidate == field)
            .ok_or_else(|| format!("Unknown field '{component}.{field}' in compiled view kernel"))?;
        self.world
            .resolve_field_address(component, field, field_index)
            .ok_or_else(|| format!("Component '{component}' has no live storage for view kernel"))
    }

    fn resolve_view_kernel_operand(
        &self,
        operand: &crate::view_kernel::ViewKernelOperand,
    ) -> Result<ResolvedViewKernelOperand, String> {
        use crate::view_kernel::ViewKernelOperand;
        Ok(match operand {
            ViewKernelOperand::Int(value) => ResolvedViewKernelOperand::Int(*value),
            ViewKernelOperand::Float(value) => ResolvedViewKernelOperand::Float(*value),
            ViewKernelOperand::Field { component, field } => ResolvedViewKernelOperand::Field(
                self.resolve_view_kernel_field(component, field)?,
            ),
        })
    }

    fn read_resolved_view_kernel_operand(
        &mut self,
        entity: u32,
        entity_address: crate::world::ResolvedEntityAddress,
        operand: ResolvedViewKernelOperand,
    ) -> Result<Value, String> {
        match operand {
            ResolvedViewKernelOperand::Int(value) => Ok(Value::from_int(&mut self.gc, value)),
            ResolvedViewKernelOperand::Float(value) => Ok(Value::from_float(value)),
            ResolvedViewKernelOperand::Field(field) => {
                let value = self
                    .world
                    .component_field_value_resolved(entity_address, field)
                    .ok_or_else(|| {
                        format!("compiled view kernel cannot read a required field on entity {entity}")
                    })?;
                Ok(if value.object_identity().is_some() {
                    value.deep_copy(&mut self.gc)
                } else {
                    value
                })
            }
        }
    }
}
