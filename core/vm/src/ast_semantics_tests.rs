use std::collections::HashMap;

#[test]
fn canonical_name_resolution_has_one_qualified_local_and_type_order() {
    let module_aliases = HashMap::from([(
        "owner".to_string(),
        HashMap::from([("Item".to_string(), "module.Item".to_string())]),
    )]);
    let local = HashMap::from([("Item".to_string(), "local.Item".to_string())]);
    let file = HashMap::from([("FileOnly".to_string(), "file.Item".to_string())]);
    let types = HashMap::from([
        (
            "module.Item".to_string(),
            "canonical.ModuleItem".to_string(),
        ),
        ("local.Item".to_string(), "canonical.LocalItem".to_string()),
        ("file.Item".to_string(), "canonical.FileItem".to_string()),
    ]);

    assert_eq!(
        crate::ast::resolve_canonical_name(
            "owner.Item",
            Some(&module_aliases),
            &[Some(&local), Some(&file)],
            &types,
        ),
        "canonical.ModuleItem"
    );
    assert_eq!(
        crate::ast::resolve_canonical_name(
            "Item",
            Some(&module_aliases),
            &[Some(&local), Some(&file)],
            &types,
        ),
        "canonical.LocalItem"
    );
    assert_eq!(
        crate::ast::resolve_canonical_name(
            "FileOnly",
            Some(&module_aliases),
            &[Some(&local), Some(&file)],
            &types,
        ),
        "canonical.FileItem"
    );
}

#[test]
fn canonical_name_resolution_is_total_for_invalid_redirect_cycles() {
    let redirects = HashMap::from([
        ("A".to_string(), "B".to_string()),
        ("B".to_string(), "A".to_string()),
    ]);
    let resolved = crate::ast::resolve_canonical_name("A", None, &[], &redirects);
    assert!(resolved == "A" || resolved == "B");
}
