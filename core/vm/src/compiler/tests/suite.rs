#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    include!("state_machines_and_numbers.rs");
    include!("calls_and_vectorization.rs");
    include!("pipeline_warnings.rs");
    include!("execution_helpers.rs");
    include!("regressions.rs");
}
