/// Build the canonical dotted name represented by a `system::` source path.
#[inline]
pub fn system_ref_qualified_string(path: &[String]) -> String {
    match path.len() {
        0 => String::new(),
        1 => path[0].clone(),
        _ => format!("{}.{}", path[0], path[1..].join(".")),
    }
}
