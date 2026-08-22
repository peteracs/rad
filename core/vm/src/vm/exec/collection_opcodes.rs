// Opcode dispatch for the collection and ECS families, plus indexing into
// lists, maps, tuples, and buffers.

impl VM {
    fn execute_collection_opcode(
        &mut self,
        op: Op,
    ) -> Result<FrameControl, crate::constraint_types::VmFailure> {
        #[allow(non_snake_case)]
        fn Err<T, E: Into<crate::constraint_types::VmFailure>>(
            error: E,
        ) -> Result<T, crate::constraint_types::VmFailure> {
            Result::Err(error.into())
        }
        match op {
            Op::Rollback => {
                self.exec_rollback()?;
            }
            Op::BitsetSetInplace => {
                self.exec_bitset_set_inplace()?;
            }
            Op::BitsetClearInplace => {
                self.exec_bitset_clear_inplace()?;
            }
            Op::BufferAppendInplace => {
                self.exec_buffer_append_inplace()?;
            }
            Op::ByteBufSetU8Inplace => {
                self.exec_bytebuf_set_u8_inplace()?;
            }
            Op::ByteBufSetU32LeInplace => {
                self.exec_bytebuf_set_u32_le_inplace()?;
            }
            Op::ByteBufSetI32LeInplace => {
                self.exec_bytebuf_set_i32_le_inplace()?;
            }
            Op::GetIter => {
                let val = self.pop()?;
                if let Some(map) = val.as_map() {
                    let map_clone = map.clone();
                    let mut sorted_keys: Vec<MapKey> = map.keys().cloned().collect();
                    sorted_keys.sort();
                    let __v = Value::map_iter(&mut self.gc, map_clone, sorted_keys);
                    self.push(__v);
                } else {
                    return Err(format!("GetIter expected map, got {}", val.type_name()));
                }
            }
            Op::IterNext => {
                let bindings_count = self.read_byte()?;
                let iter_val = self.pop()?;
                if let Some((map, idx_cell, keys)) = iter_val.as_map_iter() {
                    let idx = idx_cell.get();
                    if idx < keys.len() {
                        let k = &keys[idx];
                        let v = *map.get(k).unwrap();
                        idx_cell.set(idx + 1);

                        if bindings_count == 1 {
                            let key_v = k.to_value(&mut self.gc);
                            self.push(key_v);
                        } else {
                            let key_v = k.to_value(&mut self.gc);
                            self.push(key_v);
                            self.push(v);
                        }
                        self.push(Value::from_bool(true));
                    } else {
                        self.push(Value::from_bool(false));
                    }
                } else {
                    return Err(format!(
                        "IterNext expected map iterator, got {}",
                        iter_val.type_name()
                    ));
                }
            }
            // Stack order must match `Compiler::compile_lowered_pipeline`:
            // push item (top), then ListPushLocal <slot> — so pop item, then mutate list at slot.
            Op::ListPushLocal => {
                let slot = self.read_u16()? as usize;
                self.reject_captured_local_mutation(slot, "ListPushLocal")?;
                let elem = self.pop()?;
                let base = self.current_frame().stack_base;
                let slot_val = self
                    .stack
                    .get_mut(base.saturating_add(slot))
                    .ok_or_else(|| format!("Invalid local offset {}", slot))?;

                let mut list_val = if let Some(cell) = slot_val.as_cell() {
                    unsafe { (*cell).get() }
                } else {
                    *slot_val
                };

                if let Some(crate::value::Object::List(list)) = list_val.as_object_mut() {
                    list.push(elem);
                } else {
                    return Err(format!(
                        "ListPushLocal expected list at slot {}, got {}",
                        slot,
                        list_val.type_name()
                    ));
                }

                if let Some(cell) = slot_val.as_cell() {
                    unsafe { (*cell).set(list_val) };
                } else {
                    *slot_val = list_val;
                }
            }
            // Stack: index, value (top). Mutates the list living in the
            // local slot directly — no stack round-trip, no second Arc
            // reference, no copy-on-write clone. Only emitted for
            // `let unique` locals, whose aliasing freedom the checker
            // already guarantees.
            Op::ListSetLocal => {
                let slot = self.read_u16()? as usize;
                self.reject_captured_local_mutation(slot, "ListSetLocal")?;
                let val = self.pop()?;
                let idx_val = self.pop()?;
                let idx = helpers::index_as_usize(&idx_val)?;
                let base = self.current_frame().stack_base;
                let slot_val = self
                    .stack
                    .get_mut(base.saturating_add(slot))
                    .ok_or_else(|| format!("Invalid local offset {}", slot))?;

                let mut list_val = if let Some(cell) = slot_val.as_cell() {
                    unsafe { (*cell).get() }
                } else {
                    *slot_val
                };

                if let Some(crate::value::Object::List(list)) = list_val.as_object_mut() {
                    list.set(idx, val)?;
                } else {
                    return Err(format!(
                        "ListSetLocal expected list at slot {}, got {}",
                        slot,
                        list_val.type_name()
                    ));
                }

                if let Some(cell) = slot_val.as_cell() {
                    unsafe { (*cell).set(list_val) };
                } else {
                    *slot_val = list_val;
                }
            }
            Op::IsVariant => {
                let pattern_idx = self.read_u16()? as usize;
                let pattern = helpers::constant_string(self.current_chunk(), pattern_idx)?;
                let val = self.pop()?;
                let res = if let Some(st) = val.as_sum_type() {
                    st.variant == pattern
                } else if let Some(s) = val.as_state() {
                    s.state == pattern
                } else {
                    false
                };
                self.push(Value::from_bool(res));
            }
            Op::VecAdd => {
                self.exec_vec_binary(helpers::binary_add)?;
            }
            Op::VecSub => {
                self.exec_vec_binary(helpers::binary_sub)?;
            }
            Op::VecMul => {
                let allocation_limit = self.mem_limit;
                self.exec_vec_binary(|gc, lhs, rhs| {
                    helpers::binary_mul(gc, lhs, rhs, allocation_limit)
                })?;
            }
            Op::VecDiv => {
                self.exec_vec_binary(helpers::binary_div)?;
            }
            Op::VecMod => {
                self.exec_vec_binary(helpers::binary_mod)?;
            }
            Op::VecNeg => {
                self.exec_vec_unary(helpers::unary_neg)?;
            }
            Op::VecNot => {
                self.exec_vec_not()?;
            }
            Op::VecEq => {
                self.exec_vec_cmp(|a, b| Ok(helpers::values_equal(a, b)))?;
            }
            Op::VecNeq => {
                self.exec_vec_cmp(|a, b| Ok(!helpers::values_equal(a, b)))?;
            }
            Op::VecLt => {
                self.exec_vec_cmp(helpers::cmp_lt)?;
            }
            Op::VecGt => {
                self.exec_vec_cmp(helpers::cmp_gt)?;
            }
            Op::VecLte => {
                self.exec_vec_cmp(helpers::cmp_lte)?;
            }
            Op::VecGte => {
                self.exec_vec_cmp(helpers::cmp_gte)?;
            }
            Op::VecFilter => {
                self.exec_vec_filter()?;
            }
            Op::VecSelect => {
                self.exec_vec_select()?;
            }
            Op::LoadColumn => {
                self.exec_load_column()?;
            }
            Op::VecBroadcast => {
                self.exec_vec_broadcast()?;
            }

            Op::OnceGuardPass => {
                self.once_guard_passed = true;
            }
            _ => unreachable!("opcode dispatcher selected the wrong partition"),
        }
        Ok(FrameControl::Next)
    }

    fn execute_terminal_opcode(
        &mut self,
        op: Op,
    ) -> Result<FrameControl, crate::constraint_types::VmFailure> {
        #[allow(non_snake_case)]
        fn Err<T, E: Into<crate::constraint_types::VmFailure>>(
            error: E,
        ) -> Result<T, crate::constraint_types::VmFailure> {
            Result::Err(error.into())
        }
        match op {
            Op::PopCheckErr => {
                let val = self.pop()?;
                if let Some(st) = val.as_sum_type() {
                    if st.type_name == "Result" && st.variant == "Err" {
                        let msg = st
                            .fields
                            .get("value")
                            .or_else(|| st.fields.get("message"))
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "unknown error".to_string());
                        return Err(format!("Unhandled error from main(): {}", msg));
                    }
                    if st.type_name == "Option" && st.variant == "None" {
                        return Err("Unhandled None returned from main()".to_string());
                    }
                }
            }

            Op::Halt => {
                if self.settlement.is_some() {
                    self.guard_frame_exit("Halt")?;
                    self.abort_settlement();
                    return Err(
                        "Internal VM error: Halt cannot execute while a settlement is active"
                            .to_string(),
                    );
                }
                self.frames.clear();
                return Ok(FrameControl::Halt);
            }
            _ => unreachable!("opcode dispatcher selected the wrong partition"),
        }
        Ok(FrameControl::Next)
    }

    pub(crate) fn exec_get_index(&mut self) -> Result<(), String> {
        let idx_val = self.pop()?;
        let obj = self.pop()?;
        self.index_into(obj, idx_val)
    }

    /// Shared indexing core for GetIndex and the fused ListGetLocal.
    #[inline]
    fn index_into(&mut self, obj: Value, idx_val: Value) -> Result<(), String> {
        if let Some(items) = obj.as_list() {
            let i = helpers::index_as_usize(&idx_val)?;
            let v = items
                .get(i)
                .cloned()
                .ok_or_else(|| format!("List index {} out of bounds", i))?;
            self.push(v);
        } else if let Some(s) = obj.as_str() {
            let i = helpers::index_as_usize(&idx_val)?;
            let b = s
                .as_bytes()
                .get(i)
                .ok_or_else(|| format!("String index {} out of bounds", i))?;
            let __v = Value::from_int(&mut self.gc, *b as i64);
            self.push(__v);
        } else if let Some(t) = obj.as_tuple() {
            let i = helpers::index_as_usize(&idx_val)?;
            let v = t
                .get(i)
                .cloned()
                .ok_or_else(|| format!("Tuple index {} out of bounds (len {})", i, t.len()))?;
            self.push(v);
        } else if let Some(m) = obj.as_map() {
            let map_key = MapKey::from_value(&idx_val)?;
            let v = m.get(&map_key).cloned().unwrap_or(Value::NIL);
            self.push(v);
        } else {
            return Err(format!(
                "GetIndex expected list, string, tuple, or map, got {}",
                obj.type_name()
            ));
        }
        Ok(())
    }

    pub(crate) fn exec_set_index(&mut self) -> Result<(), String> {
        let val = self.pop()?;
        let idx_val = self.pop()?;
        let obj = self.pop()?;
        if obj.as_list().is_some() {
            let i = helpers::index_as_usize(&idx_val)?;
            let mut list = obj.into_rad_list().expect("list type already checked");
            self.meter_constraint_resources(list.len(), list.len().saturating_mul(192))?;
            list.set(i, val)?;
            let __v = Value::from_rad_list(&mut self.gc, list);
            self.push(__v);
        } else if obj.as_map().is_some() {
            let map_key = MapKey::from_value(&idx_val)?;
            let mut new_map = obj.into_map().expect("map type already checked");
            self.meter_constraint_resources(new_map.len(), new_map.len().saturating_mul(256))?;
            new_map.insert(map_key, val);
            let __v = Value::map(&mut self.gc, new_map);
            self.push(__v);
        } else {
            return Err(format!(
                "SetIndex expected list or map, got {}",
                obj.type_name()
            ));
        }
        Ok(())
    }
}
