impl VM {
    /// Charge one unit of fuel and enforce the memory ceiling.
    ///
    /// Called on loop back-edges and calls only, so any unbounded execution
    /// crosses a charge point while straight-line code stays unmetered.
    /// `u64::MAX` fuel (the default) short-circuits to a single comparison.
    #[inline(always)]
    pub(crate) fn charge_fuel(&mut self) -> Result<(), String> {
        if self.fuel == u64::MAX {
            return Ok(());
        }
        if self.fuel == 0 {
            return Err("Budget exhausted: instruction (fuel) limit reached".to_string());
        }
        self.fuel -= 1;
        if self.gc.bytes_allocated() > self.mem_limit {
            return Err(format!(
                "Budget exhausted: memory limit exceeded ({} bytes allocated)",
                self.gc.bytes_allocated()
            ));
        }
        Ok(())
    }

    /// Charge `units` of semantic work to the budget *and* the reported metric.
    ///
    /// Fused kernels do in one opcode what the equivalent unfused program does
    /// in many. `charge_fuel` alone is not enough for them: the `@budget`
    /// contract is enforced against `fuel`, but `max_instructions` is reported
    /// from `metered_instruction_count`, which the ordinary dispatch loop
    /// increments once per opcode. A kernel that only charged fuel therefore
    /// enforced a real bound while reporting 4 instructions for a system that
    /// touched 600 entities — the budget and the number a reader uses to size
    /// that budget disagreed.
    ///
    /// Routing both counters through one call makes that divergence
    /// unrepresentable: whatever a kernel charges is exactly what it reports.
    pub(crate) fn charge_work(&mut self, units: u64) -> Result<(), String> {
        if self.system_metrics.is_some() {
            self.metered_instruction_count = self.metered_instruction_count.saturating_add(units);
        }
        if self.fuel == u64::MAX {
            return Ok(());
        }
        if self.fuel < units {
            // Consume the remainder so the overrun cannot be split into
            // affordable pieces by a later charge.
            self.fuel = 0;
            return Err("Budget exhausted: instruction (fuel) limit reached".to_string());
        }
        self.fuel -= units;
        if self.gc.bytes_allocated() > self.mem_limit {
            return Err(format!(
                "Budget exhausted: memory limit exceeded ({} bytes allocated)",
                self.gc.bytes_allocated()
            ));
        }
        Ok(())
    }

    #[inline(always)]
    fn charge_constraint_instruction(&mut self) -> Result<(), String> {
        if let Some(meter) = &mut self.constraint_meter {
            meter.charge_instruction()?;
        }
        Ok(())
    }

    /// Account for an instruction consumed by a runtime superinstruction.
    /// This deliberately mirrors the outer dispatch loop so optimization
    /// cannot make fuel, model-check limits, system budgets, or opcode
    /// profiles report less semantic work than the unfused bytecode.
    #[inline(always)]
    fn charge_fused_instruction(&mut self, op: Op) -> Result<(), String> {
        self.enforce_region_opcode(op)?;
        self.charge_constraint_instruction()?;
        if self.op_profile {
            self.op_counts[op as usize] += 1;
        }
        if self.system_metrics.is_some() {
            self.metered_instruction_count = self.metered_instruction_count.saturating_add(1);
        }
        Ok(())
    }

    #[inline(always)]
    fn check_constraint_heap(&self, temporary: usize) -> Result<(), String> {
        if let Some(meter) = &self.constraint_meter {
            meter.ensure_heap(self.gc.bytes_allocated(), temporary)?;
        }
        Ok(())
    }

    #[inline]
    fn preflight_constraint_binary_allocation(
        &self,
        op: Op,
        left: &Value,
        right: &Value,
    ) -> Result<(), String> {
        let retained = std::mem::size_of::<crate::value::Object>();
        let temporary = match op {
            Op::Add => {
                if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
                    left.len()
                        .saturating_add(right.len())
                        .saturating_mul(2)
                        .saturating_add(retained)
                } else if let (Some(left), Some(right)) = (left.as_list(), right.as_list()) {
                    left.len()
                        .saturating_add(right.len())
                        .saturating_mul(std::mem::size_of::<Value>())
                        .saturating_mul(2)
                        .saturating_add(retained)
                } else {
                    0
                }
            }
            Op::Mul => {
                let repeated = if let (Some(text), Some(count)) = (left.as_str(), right.as_int()) {
                    (count >= 0).then(|| text.len().saturating_mul(count as usize))
                } else if let (Some(count), Some(text)) = (left.as_int(), right.as_str()) {
                    (count >= 0).then(|| text.len().saturating_mul(count as usize))
                } else {
                    None
                };
                repeated
                    .unwrap_or(0)
                    .saturating_mul(2)
                    .saturating_add(retained)
            }
            _ => 0,
        };
        self.check_constraint_heap(temporary)
    }

    /// Collect floating garbage when the heap crosses its growth threshold.
    ///
    /// Polled at loop back-edges and calls — the same points that charge
    /// fuel — so straight-line code pays one load+cmp and any program that
    /// allocates without bound crosses a collection point. Without this, a
    /// long-running server that never calls `gc_collect()` accretes every
    /// transient payload it ever built (the syncdesk soak hit 3 GB in 50 s).
    ///
    /// Metered VMs (sandboxes) are exempt: their `mem_bytes` cap is a
    /// *total allocation* budget that doubles as a work bound, and
    /// collecting garbage out from under it would quietly change it into a
    /// (much slower to trip) live-memory cap. Sandboxed code can still call
    /// `gc_collect()` if granted.
    #[inline(always)]
    fn maybe_gc(&mut self) {
        // `gc_pause`: a builtin is holding heap values in Rust locals across
        // this nested execution (simulate's saved timeline, decode-path
        // migrations) — the collector cannot see them as roots.
        if self.mem_limit == usize::MAX && self.gc_pause == 0 && self.gc.should_collect() {
            self.collect_cycles();
        }
    }

    /// Enforce the sandbox component-write ACL. No-op for trusted code.
    #[inline]
    pub(crate) fn sandbox_check_write(&self, component: &str) -> Result<(), String> {
        self.sandbox_check_named_write("component", component)
    }

    /// Enforce the same deny-by-default write grant for an authoritative
    /// relation identity. Grants name the world type they permit; component
    /// and relation mutation share one capability mechanism.
    #[inline]
    pub(crate) fn sandbox_check_relation_write(&self, relation: &str) -> Result<(), String> {
        self.sandbox_check_named_write("relation", relation)
    }

    #[inline]
    fn sandbox_check_named_write(&self, kind: &str, identity: &str) -> Result<(), String> {
        if let Some(trace) = &self.model_access_trace {
            trace.borrow_mut().writes.insert(identity);
        }
        if let Some(caps) = &self.sandbox_caps {
            if !caps.may_write(identity) {
                return Err(format!(
                    "sandbox: write to {kind} '{identity}' denied by capability grant"
                ));
            }
        }
        Ok(())
    }

    /// Enforce the sandbox component-read ACL (confidentiality dimension).
    /// No-op for trusted code, and no-op for any grant without an explicit
    /// `"read"` key (those read everything). Mirrors `sandbox_check_write`.
    #[inline]
    pub(crate) fn sandbox_check_read(&self, component: &str) -> Result<(), String> {
        if let Some(trace) = &self.model_access_trace {
            trace.borrow_mut().reads.insert(component);
        }
        if let Some(caps) = &self.sandbox_caps {
            if !caps.may_read(component) {
                return Err(format!(
                    "sandbox: read of component '{}' denied by capability grant",
                    component
                ));
            }
        }
        Ok(())
    }

    /// A whole-world reader (`save_world`, `world_digest`, unfiltered
    /// `entities()`) cannot be keyed to one component, so it requires the
    /// wildcard read grant — the confidentiality mirror of
    /// `sandbox_check_despawn`. No-op for trusted code.
    #[inline]
    pub(crate) fn sandbox_check_bulk_read(&self, what: &str) -> Result<(), String> {
        if let Some(trace) = &self.model_access_trace {
            trace.borrow_mut().reads.insert("*");
        }
        if let Some(caps) = &self.sandbox_caps {
            if !caps.may_read_all() {
                return Err(format!(
                    "sandbox: {} reads all world state and requires the \"*\" read grant",
                    what
                ));
            }
        }
        Ok(())
    }

    /// Despawning touches every component on the entity, so it requires the
    /// wildcard (`"*"`) grant. No-op for trusted code.
    #[inline]
    pub(crate) fn sandbox_check_despawn(&self) -> Result<(), String> {
        if let Some(trace) = &self.model_access_trace {
            trace.borrow_mut().writes.insert("*");
        }
        if let Some(caps) = &self.sandbox_caps {
            if !caps.may_despawn() {
                return Err(
                    "sandbox: despawn denied by capability grant (requires the \"*\" write grant)"
                        .to_string(),
                );
            }
        }
        Ok(())
    }
}
