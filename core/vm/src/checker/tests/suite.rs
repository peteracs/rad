#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    include!("spans_and_state_machines.rs");
    include!("systems_and_types.rs");
    include!("functions_and_variants.rs");
    include!("variants_and_statements.rs");
    include!("closures_and_entrypoints.rs");
    include!("effects_and_mutability.rs");
    include!("readonly_and_recursive_bindings.rs");
}
