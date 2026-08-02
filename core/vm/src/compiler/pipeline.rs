use super::{CompileError, Compiler};
use crate::ast::Expr;
use crate::opcode::Op;
use crate::value::{PipelineOp, Value};

impl Compiler {
    /// Lowers a pipeline into a single loop.
    pub(crate) fn compile_lowered_pipeline(
        &mut self,
        source: &Expr,
        steps: &[(&Expr, PipelineOp)],
        line: u32,
    ) -> Result<(), CompileError> {
        self.begin_scope();

        // 1. Compile source list
        self.compile_expr(source)?;
        let list_name = self.fresh_name("pipe_list");
        self.add_local(list_name.clone(), false);
        let list_slot = self.resolve_local(&list_name).unwrap();

        // 2. Create the result list
        self.emit_op(Op::MakeList, line);
        self.emit_u16(0, line);
        let result_name = self.fresh_name("pipe_result");
        self.add_local(result_name.clone(), true);
        let result_slot = self.resolve_local(&result_name).unwrap();

        // 3. Create loop index
        self.emit_constant_gc(line, |gc| Value::from_int(gc, 0));
        let idx_name = self.fresh_name("pipe_idx");
        self.add_local(idx_name.clone(), true);
        let idx_slot = self.resolve_local(&idx_name).unwrap();

        // 4. Loop start
        let loop_start = self.current_offset();
        self.mark_label_here();

        // if idx < len(list)
        self.emit_get_local(idx_slot, line);
        self.emit_get_local(list_slot, line);
        self.emit_op(Op::Len, line);
        self.emit_op(Op::Lt, line);
        let exit_jump = self.emit_jump(Op::JumpIfFalse, line);

        self.begin_scope();

        // let item = list[idx]
        self.emit_get_local(list_slot, line);
        self.emit_get_local(idx_slot, line);
        self.emit_op(Op::GetIndex, line);

        let item_name = self.fresh_name("pipe_item");
        self.add_local(item_name.clone(), true);
        let item_slot = self.resolve_local(&item_name).unwrap();

        let mut skip_holes = Vec::new();

        for (func_expr, op) in steps.iter() {
            let is_inlined =
                if let Expr::FnExpr(params, _, _, param_destructures, _, body, _) = func_expr {
                    params.len() == 1
                        && param_destructures.iter().all(|d| d.is_none())
                        && body.stmts.len() == 1
                        && matches!(body.stmts[0], crate::ast::Stmt::Expr(_))
                } else {
                    false
                };

            match op {
                PipelineOp::Map => {
                    if is_inlined {
                        if let Expr::FnExpr(params, _, _, _, _, body, _) = func_expr {
                            if let crate::ast::Stmt::Expr(expr_stmt) = &body.stmts[0] {
                                self.begin_scope();
                                self.emit_get_local(item_slot, line);
                                self.add_local(params[0].clone(), false);
                                self.compile_expr(&expr_stmt.expr)?;
                                self.end_scope_keep_top(line);
                                self.emit_op(Op::SetLocal, line);
                                self.emit_u16(item_slot, line);
                            }
                        }
                    } else {
                        self.emit_get_local(item_slot, line);
                        self.compile_expr(func_expr)?;
                        self.emit_op(Op::Call, line);
                        self.emit_byte(1, line);
                        self.emit_op(Op::SetLocal, line);
                        self.emit_u16(item_slot, line);
                    }
                }
                PipelineOp::Filter => {
                    if is_inlined {
                        if let Expr::FnExpr(params, _, _, _, _, body, _) = func_expr {
                            if let crate::ast::Stmt::Expr(expr_stmt) = &body.stmts[0] {
                                self.begin_scope();
                                self.emit_get_local(item_slot, line);
                                self.add_local(params[0].clone(), false);
                                self.compile_expr(&expr_stmt.expr)?;
                                self.end_scope_keep_top(line);
                            }
                        }
                    } else {
                        self.emit_get_local(item_slot, line);
                        self.compile_expr(func_expr)?;
                        self.emit_op(Op::Call, line);
                        self.emit_byte(1, line);
                    }
                    let skip_jump = self.emit_jump(Op::JumpIfFalse, line);
                    skip_holes.push(skip_jump);
                }
            }
        }

        // We have the final item in `item_slot`. Push it to the result list.
        self.emit_get_local(item_slot, line);
        self.emit_op(Op::ListPushLocal, line);
        self.emit_u16(result_slot, line);

        // Patch skip holes (from filter)
        let continue_target = self.current_offset();
        self.mark_label_here();
        for hole in skip_holes {
            self.patch_jump_to(hole, continue_target);
        }

        self.end_scope(line); // Pops the item and any locals created in the loop body

        // idx = idx + 1
        self.emit_get_local(idx_slot, line);
        self.emit_constant_gc(line, |gc| Value::from_int(gc, 1));
        self.emit_op(Op::Add, line);
        self.emit_op(Op::SetLocal, line);
        self.emit_u16(idx_slot, line);

        let loop_end = self.current_offset();
        let delta = loop_end - loop_start + 3;
        self.emit_op(Op::JumpBack, line);
        self.emit_u16(delta as u16, line);

        self.patch_jump(exit_jump);

        self.emit_get_local(result_slot, line);
        self.end_scope_keep_top(line);
        Ok(())
    }
}
