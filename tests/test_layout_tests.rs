/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::{
    fs,
    path::Path,
};

fn declared_module_name(line: &str) -> Option<&str> {
    let line = line.trim();
    let rest = line.strip_prefix("pub mod ").or_else(|| line.strip_prefix("mod "))?;
    let module_name = rest.strip_suffix(';')?.trim();
    if module_name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        Some(module_name)
    } else {
        None
    }
}

#[test]
fn test_top_level_integration_targets_are_not_declared_from_aggregate_mod() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let test_root = project_root.join("tests");
    let mod_file = test_root.join("mod.rs");
    let mod_content = fs::read_to_string(&mod_file).expect("tests/mod.rs should be readable for layout check");

    let duplicates = mod_content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let module_name = declared_module_name(line)?;
            let target_file = test_root.join(format!("{}.rs", module_name));
            target_file
                .is_file()
                .then(|| format!("line {} declares tests/{}.rs", index + 1, module_name))
        })
        .collect::<Vec<_>>();

    assert!(
        duplicates.is_empty(),
        "tests/mod.rs declares top-level integration test targets that Cargo \
         already runs separately: {}",
        duplicates.join(", ")
    );
}
