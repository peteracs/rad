use super::*;
use crate::view_kernel::{
    ViewKernelArithmetic, ViewKernelOperand, ViewKernelPlan, ViewKernelWrite,
};

#[derive(Clone, Debug)]
enum KernelSymbol {
    Entity,
    Operand(ViewKernelOperand),
    Arithmetic {
        left: ViewKernelOperand,
        operation: ViewKernelArithmetic,
        right: ViewKernelOperand,
    },
    Nil,
}

impl Compiler {
    /// Lower a statically resolved allocation-free view callback to one data
    /// kernel. Returning `false` means the callback is valid but not in the
    /// deliberately small, proof-carrying arithmetic subset; ordinary call
    /// lowering then preserves its exact behavior.
    pub(super) fn try_compile_view_kernel(
        &mut self,
        args: &[Expr],
        span: &Span,
    ) -> Result<bool, CompileError> {
        if args.len() != 2 || self.in_causal_region() {
            return Ok(false);
        }
        let Some(view) = self.static_kernel_name(&args[0]) else {
            return Ok(false);
        };
        let Some(callback) = self.static_kernel_name(&args[1]) else {
            return Ok(false);
        };

        let mut call_stack = Vec::new();
        let Some(writes) =
            self.analyze_kernel_function(&callback, vec![KernelSymbol::Entity], &mut call_stack)
        else {
            return Ok(false);
        };
        if writes.is_empty() {
            return Ok(false);
        }
        if writes.iter().any(|write| {
            self.indexed_kernel_fields
                .contains(&(write.component.clone(), write.field.clone()))
        }) {
            return Ok(false);
        }

        let kernel_index = u16::try_from(self.view_kernels.len()).map_err(|_| CompileError {
            message: "A program may contain at most 65,536 compiled view kernels".to_string(),
            line: span.line,
            col: span.col,
        })?;
        self.view_kernels.push(ViewKernelPlan { view, writes });
        self.emit_op(Op::RunViewKernel, span.line);
        self.emit_u16(kernel_index, span.line);
        Ok(true)
    }

    fn static_kernel_name(&self, expression: &Expr) -> Option<String> {
        match expression {
            Expr::Ident(name, _) => Some(self.resolve_canonical_name(name)),
            Expr::Field(owner, member, _) if matches!(owner.as_ref(), Expr::Ident(_, _)) => {
                let Expr::Ident(alias, _) = owner.as_ref() else {
                    unreachable!()
                };
                self.resolve_alias_member(alias, member)
            }
            _ => None,
        }
    }

    fn analyze_kernel_function(
        &mut self,
        name: &str,
        arguments: Vec<KernelSymbol>,
        call_stack: &mut Vec<String>,
    ) -> Option<Vec<ViewKernelWrite>> {
        let resolved = self.resolve_canonical_name(name);
        let declaration = self
            .function_declarations
            .get(&resolved)
            .or_else(|| self.function_declarations.get(name))
            .cloned()?;
        if declaration.is_async
            || declaration.params.len() != arguments.len()
            || call_stack.iter().any(|active| active == &resolved)
        {
            return None;
        }

        let previous_scope = self.current_file_scope.clone();
        self.current_file_scope = declaration
            .span
            .file
            .and_then(|file| self.file_private_scopes.get(&file.0).cloned());
        call_stack.push(resolved);

        let mut symbols = declaration
            .params
            .iter()
            .cloned()
            .zip(arguments)
            .collect::<HashMap<_, _>>();
        let mut writes = Vec::new();
        let valid =
            self.analyze_kernel_block(&declaration.body, &mut symbols, &mut writes, call_stack);

        call_stack.pop();
        self.current_file_scope = previous_scope;
        valid.then_some(writes)
    }

    fn analyze_kernel_block(
        &mut self,
        block: &Block,
        symbols: &mut HashMap<String, KernelSymbol>,
        writes: &mut Vec<ViewKernelWrite>,
        call_stack: &mut Vec<String>,
    ) -> bool {
        for statement in &block.stmts {
            match statement {
                Stmt::Let(binding)
                    if binding.names.len() == 1
                        && !binding.tuple_destructure
                        && !binding.recursive =>
                {
                    let Some(value) = self.analyze_kernel_expression(&binding.value, symbols)
                    else {
                        return false;
                    };
                    symbols.insert(binding.names[0].clone(), value);
                }
                Stmt::Expr(expression) => {
                    if !self.analyze_kernel_effect(&expression.expr, symbols, writes, call_stack) {
                        return false;
                    }
                }
                Stmt::Return(return_statement) => {
                    if let Some(value) = &return_statement.value {
                        if !matches!(
                            self.analyze_kernel_expression(value, symbols),
                            Some(KernelSymbol::Nil)
                        ) {
                            return false;
                        }
                    }
                    break;
                }
                _ => return false,
            }
        }
        true
    }

    fn analyze_kernel_effect(
        &mut self,
        expression: &Expr,
        symbols: &HashMap<String, KernelSymbol>,
        writes: &mut Vec<ViewKernelWrite>,
        call_stack: &mut Vec<String>,
    ) -> bool {
        let Expr::Call(callee, args, _) = expression else {
            return false;
        };
        let Some(callee_name) = self.static_kernel_name(callee) else {
            return false;
        };
        if callee_name == "write_field" {
            if args.len() != 4
                || !matches!(
                    self.analyze_kernel_expression(&args[0], symbols),
                    Some(KernelSymbol::Entity)
                )
            {
                return false;
            }
            let Some(component) = self.static_kernel_name(&args[1]) else {
                return false;
            };
            let Expr::StrLit(field, _) = &args[2] else {
                return false;
            };
            let Some(KernelSymbol::Arithmetic {
                left,
                operation,
                right,
            }) = self.analyze_kernel_expression(&args[3], symbols)
            else {
                return false;
            };
            writes.push(ViewKernelWrite {
                component,
                field: field.clone(),
                left,
                operation,
                right,
            });
            return true;
        }

        let mut arguments = Vec::with_capacity(args.len());
        for argument in args {
            let Some(argument) = self.analyze_kernel_expression(argument, symbols) else {
                return false;
            };
            arguments.push(argument);
        }
        let Some(mut nested) = self.analyze_kernel_function(&callee_name, arguments, call_stack)
        else {
            return false;
        };
        writes.append(&mut nested);
        true
    }

    fn analyze_kernel_expression(
        &self,
        expression: &Expr,
        symbols: &HashMap<String, KernelSymbol>,
    ) -> Option<KernelSymbol> {
        match expression {
            Expr::Ident(name, _) => symbols.get(name).cloned(),
            Expr::IntLit(value, _) => Some(KernelSymbol::Operand(ViewKernelOperand::Int(*value))),
            Expr::FloatLit(value, _) => {
                Some(KernelSymbol::Operand(ViewKernelOperand::Float(*value)))
            }
            Expr::NilLit(_) => Some(KernelSymbol::Nil),
            Expr::Binary(left, operation, right, _) => {
                let KernelSymbol::Operand(left) = self.analyze_kernel_expression(left, symbols)?
                else {
                    return None;
                };
                let KernelSymbol::Operand(right) =
                    self.analyze_kernel_expression(right, symbols)?
                else {
                    return None;
                };
                let operation = match operation {
                    BinOp::Add => ViewKernelArithmetic::Add,
                    BinOp::Sub => ViewKernelArithmetic::Sub,
                    BinOp::Mul => ViewKernelArithmetic::Mul,
                    BinOp::Div => ViewKernelArithmetic::Div,
                    _ => return None,
                };
                Some(KernelSymbol::Arithmetic {
                    left,
                    operation,
                    right,
                })
            }
            Expr::Call(callee, args, _) => {
                let name = self.static_kernel_name(callee)?;
                if name != "read_field"
                    || args.len() != 3
                    || !matches!(
                        self.analyze_kernel_expression(&args[0], symbols),
                        Some(KernelSymbol::Entity)
                    )
                {
                    return None;
                }
                let component = self.static_kernel_name(&args[1])?;
                let Expr::StrLit(field, _) = &args[2] else {
                    return None;
                };
                Some(KernelSymbol::Operand(ViewKernelOperand::Field {
                    component,
                    field: field.clone(),
                }))
            }
            _ => None,
        }
    }
}
