struct OperationalProgram {
    authority: rad_vm::types::AuthorityReport,
    compiled: rad_vm::compiler::CompileResult,
}

#[derive(Clone, Copy)]
struct AllocationContractAssessment {
    guest_declared: bool,
    guest_pass: bool,
    runtime_declared: bool,
    runtime_pass: bool,
    host_declared: bool,
    host_pass: bool,
}

impl AllocationContractAssessment {
    fn all_pass(self) -> bool {
        self.guest_pass && self.runtime_pass && self.host_pass
    }
}

fn assess_allocation_contracts(
    contracts: Option<&rad_vm::types::AuthorityContracts>,
    static_allocation: bool,
    metric: &rad_vm::vm::SystemExecutionMetrics,
) -> AllocationContractAssessment {
    let guest_declared = contracts.is_some_and(|value| value.no_guest_allocation);
    let runtime_declared = contracts.is_some_and(|value| value.no_runtime_allocation);
    let host_declared = contracts.is_some_and(|value| value.no_host_allocation);
    AllocationContractAssessment {
        guest_declared,
        guest_pass: !guest_declared
            || (metric.native_allocation_meter_supported
            && !static_allocation
            && metric.max_guest_allocations == 0
                && metric.max_managed_backing_allocations == 0),
        runtime_declared,
        runtime_pass: !runtime_declared
            || (metric.native_allocation_meter_supported && metric.max_runtime_allocations == 0),
        host_declared,
        host_pass: !host_declared
            || (metric.native_allocation_meter_supported
                && metric.max_host_boundary_allocations == 0),
    }
}

fn run_bench_command(filepath: String, json: bool, program_args: Vec<String>) {
    let inspected = match load_operational_program(&filepath) {
        Ok(inspected) => inspected,
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    };
    let authority = inspected.authority;
    let mut vm = VM::new();
    if json {
        vm.suppress_output();
    }
    vm.enable_system_metrics();
    vm.sys_args = program_args;
    vm.load_compile_result(inspected.compiled);
    let started = std::time::Instant::now();
    if let Err(error) = vm.run(0) {
        eprintln!("Benchmark program failed: {error}");
        process::exit(1);
    }
    let elapsed = started.elapsed();
    let metrics = vm
        .system_execution_metrics()
        .expect("benchmark metrics remain enabled");
    let failed_contracts = metrics
        .iter()
        .filter_map(|(name, metric)| {
            let checked = authority.resolve(name).ok();
            let static_allocation = checked.is_some_and(|item| item.transitive.allocates);
            let allocation = assess_allocation_contracts(
                checked.map(|item| &item.contracts),
                static_allocation,
                metric,
            );
            let budget_failed = metric
                .instruction_budget
                .is_some_and(|budget| metric.max_instructions > budget);
            let allocation_failed = !allocation.all_pass();
            (budget_failed || allocation_failed).then(|| name.clone())
        })
        .collect::<Vec<_>>();

    if json {
        let systems = metrics
            .iter()
            .map(|(name, metric)| {
                let checked = authority.resolve(name).ok();
                let static_allocation = checked.is_some_and(|item| item.transitive.allocates);
                let allocation = assess_allocation_contracts(
                    checked.map(|item| &item.contracts),
                    static_allocation,
                    metric,
                );
                serde_json::json!({
                    "name": name,
                    "invocations": metric.invocations,
                    "total_instructions": metric.total_instructions,
                    "max_instructions": metric.max_instructions,
                    "instruction_budget": metric.instruction_budget,
                    "instruction_budget_pass": metric.instruction_budget.is_none_or(|budget| metric.max_instructions <= budget),
                    "total_guest_allocations": metric.total_guest_allocations,
                    "max_guest_allocations": metric.max_guest_allocations,
                    "total_guest_allocated_bytes": metric.total_guest_allocated_bytes,
                    "max_guest_allocated_bytes": metric.max_guest_allocated_bytes,
                    "native_allocation_meter_supported": metric.native_allocation_meter_supported,
                    "total_runtime_allocations": metric.total_runtime_allocations,
                    "max_runtime_allocations": metric.max_runtime_allocations,
                    "total_runtime_allocated_bytes": metric.total_runtime_allocated_bytes,
                    "max_runtime_allocated_bytes": metric.max_runtime_allocated_bytes,
                    "total_managed_backing_allocations": metric.total_managed_backing_allocations,
                    "max_managed_backing_allocations": metric.max_managed_backing_allocations,
                    "total_managed_backing_bytes": metric.total_managed_backing_bytes,
                    "max_managed_backing_bytes": metric.max_managed_backing_bytes,
                    "total_host_boundary_allocations": metric.total_host_boundary_allocations,
                    "max_host_boundary_allocations": metric.max_host_boundary_allocations,
                    "total_host_boundary_allocated_bytes": metric.total_host_boundary_allocated_bytes,
                    "max_host_boundary_allocated_bytes": metric.max_host_boundary_allocated_bytes,
                    "static_allocation_reachable": static_allocation,
                    "no_guest_allocation_declared": allocation.guest_declared,
                    "no_guest_allocation_pass": allocation.guest_pass,
                    "no_runtime_allocation_declared": allocation.runtime_declared,
                    "no_runtime_allocation_pass": allocation.runtime_pass,
                    "no_host_allocation_declared": allocation.host_declared,
                    "no_host_allocation_pass": allocation.host_pass,
                })
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "file": filepath,
                "elapsed_ns": elapsed.as_nanos(),
                "systems": systems,
                "world_digest": vm.world_digest(),
            }))
            .expect("benchmark report is serializable")
        );
        if failed_contracts.is_empty() {
            return;
        }
        eprintln!(
            "Benchmark contracts failed for: {}",
            failed_contracts.join(", ")
        );
        process::exit(1);
    }

    println!("benchmark {}: {:.3}s", filepath, elapsed.as_secs_f64());
    if metrics.is_empty() {
        println!("  systems: none executed");
    }
    for (name, metric) in metrics {
        let checked = authority.resolve(name).ok();
        let static_allocation = checked.is_some_and(|item| item.transitive.allocates);
        let allocation = assess_allocation_contracts(
            checked.map(|item| &item.contracts),
            static_allocation,
            metric,
        );
        let budget = metric.instruction_budget.map_or_else(
            || "none".to_string(),
            |budget| {
                format!(
                    "{budget} (PASS: max {} <= {budget})",
                    metric.max_instructions
                )
            },
        );
        let guest_proof = if allocation.guest_declared {
            if allocation.guest_pass {
                "PASS (static path none; guest objects and managed backing max 0)"
            } else if !metric.native_allocation_meter_supported {
                "FAIL (native allocation meter unavailable)"
            } else {
                "FAIL"
            }
        } else {
            "not declared"
        };
        let runtime_proof = if allocation.runtime_declared {
            if allocation.runtime_pass {
                "PASS (runtime allocator max 0)"
            } else if !metric.native_allocation_meter_supported {
                "FAIL (native allocation meter unavailable)"
            } else {
                "FAIL"
            }
        } else {
            "not declared"
        };
        let host_proof = if allocation.host_declared {
            if allocation.host_pass {
                "PASS (host-boundary allocator max 0)"
            } else if !metric.native_allocation_meter_supported {
                "FAIL (native allocation meter unavailable)"
            } else {
                "FAIL"
            }
        } else {
            "not declared"
        };
        println!(
            "  system {name}\n    invocations: {}\n    instructions: total {}, max {}, budget {}\n    guest GC objects: total {}, max {}, accounted bytes total {}, max {}\n    runtime allocator: calls total {}, max {}, requested bytes total {}, max {}\n    managed GC backing: calls total {}, max {}, requested bytes total {}, max {}\n    host boundary allocator: calls total {}, max {}, requested bytes total {}, max {}\n    native meter: {}\n    @no_guest_allocation: {}\n    @no_runtime_allocation: {}\n    @no_host_allocation: {}",
            metric.invocations,
            metric.total_instructions,
            metric.max_instructions,
            budget,
            metric.total_guest_allocations,
            metric.max_guest_allocations,
            metric.total_guest_allocated_bytes,
            metric.max_guest_allocated_bytes,
            metric.total_runtime_allocations,
            metric.max_runtime_allocations,
            metric.total_runtime_allocated_bytes,
            metric.max_runtime_allocated_bytes,
            metric.total_managed_backing_allocations,
            metric.max_managed_backing_allocations,
            metric.total_managed_backing_bytes,
            metric.max_managed_backing_bytes,
            metric.total_host_boundary_allocations,
            metric.max_host_boundary_allocations,
            metric.total_host_boundary_allocated_bytes,
            metric.max_host_boundary_allocated_bytes,
            if metric.native_allocation_meter_supported {
                "available"
            } else {
                "unavailable"
            },
            guest_proof,
            runtime_proof,
            host_proof,
        );
    }
    println!("  world digest: {}", vm.world_digest());
    if !failed_contracts.is_empty() {
        eprintln!(
            "Benchmark contracts failed for: {}",
            failed_contracts.join(", ")
        );
        process::exit(1);
    }
}

fn run_operational_command(query: OperationalQuery, filepath: String, json: bool) {
    let inspected = match load_operational_program(&filepath) {
        Ok(inspected) => inspected,
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    };

    let output: Result<String, String> = (|| match query {
        OperationalQuery::QueryPlan { target } => render_query_plan(&target, &inspected, json),
        OperationalQuery::CostPath { target } => {
            render_cost_path(&target, &inspected.authority, json)
        }
        OperationalQuery::Why { entity, component } => {
            let vm = run_inspected_program(inspected)?;
            let entity = resolve_operational_entity(&vm, &entity)?;
            let explanation = vm
                .causality_ledger()
                .explain_entity(entity, &component, u64::MAX);
            Ok(if json {
                serde_json::to_string_pretty(&serde_json::json!({
                    "entity": entity,
                    "component": component,
                    "explanation": explanation,
                }))
                .expect("provenance result is serializable")
            } else {
                explanation
            })
        }
        OperationalQuery::WhyField {
            entity,
            component,
            field,
        } => {
            let vm = run_inspected_program(inspected)?;
            let entity = resolve_operational_entity(&vm, &entity)?;
            let explanation = vm
                .causality_ledger()
                .explain_field(entity, &component, &field, u64::MAX);
            Ok(if json {
                serde_json::to_string_pretty(&serde_json::json!({
                    "entity": entity,
                    "component": component,
                    "field": field,
                    "explanation": explanation,
                }))
                .expect("field provenance result is serializable")
            } else {
                explanation
            })
        }
        OperationalQuery::WhyRemoved { entity, component } => {
            let vm = run_inspected_program(inspected)?;
            let entity = resolve_operational_entity(&vm, &entity)?;
            let explanation = vm.causality_ledger().explain_removed(entity, &component);
            Ok(if json {
                serde_json::to_string_pretty(&serde_json::json!({
                    "entity": entity,
                    "component": component,
                    "explanation": explanation,
                }))
                .expect("provenance result is serializable")
            } else {
                explanation
            })
        }
        OperationalQuery::WhyNotInView { view, entity } => {
            let vm = run_inspected_program(inspected)?;
            let entity = resolve_operational_entity(&vm, &entity)?;
            let (member, reason) = vm
                .get_world()
                .materialized_view_reason(&view, entity)
                .ok_or_else(|| format!("unknown materialized view '{view}'"))?;
            if member {
                Err(format!("entity {entity} is in {view}: {reason}"))
            } else if json {
                Ok(serde_json::to_string_pretty(&serde_json::json!({
                    "view": view,
                    "entity": entity,
                    "member": false,
                    "reason": reason,
                }))
                .expect("view provenance is serializable"))
            } else {
                Ok(format!("entity {entity} is not in {view}: {reason}"))
            }
        }
    })();

    match output {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("operational query: {error}");
            process::exit(1);
        }
    }
}

fn load_operational_program(filepath: &str) -> Result<OperationalProgram, String> {
    let checked = load_checked_cli_program(filepath)?;
    Ok(OperationalProgram {
        authority: checked.authority,
        compiled: checked.compiled,
    })
}

fn run_inspected_program(inspected: OperationalProgram) -> Result<VM, String> {
    let mut vm = VM::new();
    vm.suppress_output();
    vm.load_compile_result(inspected.compiled);
    vm.run(0)
        .map_err(|error| format!("program failed before inspection: {error}"))?;
    Ok(vm)
}

fn resolve_operational_entity(vm: &VM, selector: &str) -> Result<u32, String> {
    if let Some(entity) = vm.get_world().get_entity_by_name(selector) {
        return Ok(entity);
    }
    let entity = selector.parse::<u32>().map_err(|_| {
        format!("unknown entity '{selector}'; use an exact entity name or numeric entity id")
    })?;
    vm.get_world()
        .entity_exists(entity)
        .then_some(entity)
        .ok_or_else(|| format!("entity {entity} does not exist after program execution"))
}

fn resolve_operational_view<'a>(
    target: &str,
    views: &'a [rad_vm::compiler::MaterializedViewInfo],
) -> Result<Option<&'a rad_vm::compiler::MaterializedViewInfo>, String> {
    let mut matches = views
        .iter()
        .filter(|view| view.name == target || view.name.rsplit("__").next() == Some(target))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.name.cmp(&right.name));
    match matches.as_slice() {
        [] => Ok(None),
        [view] => Ok(Some(*view)),
        many => Err(format!(
            "materialized view '{target}' is ambiguous: {}",
            many.iter()
                .map(|view| view.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn render_query_plan(
    target: &str,
    inspected: &OperationalProgram,
    json: bool,
) -> Result<String, String> {
    if let Some(view) = resolve_operational_view(target, &inspected.compiled.materialized_views)? {
        let predicates = view
            .predicate
            .clauses
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if json {
            return Ok(serde_json::to_string_pretty(&serde_json::json!({
                "target": view.name,
                "kind": "materialized_view",
                "dependencies": view.dependencies,
                "key": view.key.as_ref().map(|(component, field)| serde_json::json!({
                    "component": component,
                    "field": field,
                })),
                "predicates": predicates,
                "maintenance": "incremental_same_commit",
                "operators": [
                    {"operation": "visit_view", "complexity": "O(k)", "allocates": false},
                    {"operation": "entities", "complexity": "O(k)", "allocates": true}
                ],
                "full_scans": 0,
                "sorting": 0,
            }))
            .expect("view plan is serializable"));
        }
        let key = view.key.as_ref().map_or_else(
            || "none".to_string(),
            |(component, field)| format!("{component}.{field}"),
        );
        return Ok(format!(
            "query plan {}\n  kind: materialized view\n  dependencies: {}\n  key: {}\n  predicates: {}\n  maintenance: incremental, same commit\n  visit_view: O(k), allocations 0\n  entities(view): O(k), allocations 1 result list\n  full scans: 0\n  sorting: 0",
            view.name,
            view.dependencies.join(", "),
            key,
            if predicates.is_empty() {
                "none".to_string()
            } else {
                predicates.join(" and ")
            },
        ));
    }

    let callable = inspected.authority.resolve(target)?;
    let scan_path = inspected.authority.full_scan_path(target)?;
    let allocation_path = inspected.authority.allocation_path(target)?;
    if json {
        return Ok(serde_json::to_string_pretty(&serde_json::json!({
            "target": callable.display_name,
            "kind": callable.kind,
            "queries": callable.transitive.queries,
            "reads": callable.transitive.reads,
            "full_scan": callable.transitive.full_scan,
            "full_scan_path": scan_path,
            "allocates": callable.transitive.allocates,
            "allocation_path": allocation_path,
            "contracts": callable.contracts,
        }))
        .expect("callable query plan is serializable"));
    }
    let operations = callable
        .transitive
        .queries
        .iter()
        .map(|operation| {
            format!(
                "    {} {}: {}, allocates {}",
                operation.operation, operation.source, operation.complexity, operation.allocates
            )
        })
        .collect::<Vec<_>>();
    Ok(format!(
        "query plan {}\n  operations:\n{}\n  full scan: {}\n  guest allocation reachable: {}\n  contracts: @frame={} @no_full_scan={} @no_guest_allocation={} @no_runtime_allocation={} @no_host_allocation={} @budget={}",
        callable.display_name,
        if operations.is_empty() {
            "    none".to_string()
        } else {
            operations.join("\n")
        },
        callable.transitive.full_scan,
        callable.transitive.allocates,
        callable.contracts.frame,
        callable.contracts.no_full_scan,
        callable.contracts.no_guest_allocation,
        callable.contracts.no_runtime_allocation,
        callable.contracts.no_host_allocation,
        callable
            .contracts
            .instruction_budget
            .map_or_else(|| "none".to_string(), |budget| budget.to_string()),
    ))
}

fn render_cost_path(
    target: &str,
    authority: &rad_vm::types::AuthorityReport,
    json: bool,
) -> Result<String, String> {
    let callable = authority.resolve(target)?;
    let full_scan = authority.full_scan_path(target)?;
    let allocation = authority.allocation_path(target)?;
    if json {
        return Ok(serde_json::to_string_pretty(&serde_json::json!({
            "target": callable.display_name,
            "full_scan": full_scan,
            "allocation": allocation,
            "queries": callable.transitive.queries,
        }))
        .expect("cost path is serializable"));
    }
    Ok(format!(
        "cost path {}\n  full scan: {}\n  allocation: {}",
        callable.display_name,
        full_scan.map_or_else(|| "none".to_string(), |path| path.join(" -> ")),
        allocation.map_or_else(|| "none".to_string(), |path| path.join(" -> ")),
    ))
}
