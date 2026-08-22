#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    include!("state_machines_and_numbers.rs");
    include!("calls_and_vectorization.rs");
    include!("pipeline_warnings.rs");
    include!("authority_execution.rs");
    include!("execution_helpers.rs");
    include!("transactions.rs");
    include!("native_types.rs");
    include!("semantic_features.rs");
    include!("regressions.rs");
}
