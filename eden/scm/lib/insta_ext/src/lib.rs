/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub fn setup() {
    const WORKSPACE: &str = "INSTA_WORKSPACE_ROOT";
    const UPDATE: &str = "INSTA_UPDATE";
    if std::env::var(WORKSPACE).is_err() {
        let mut root = std::path::PathBuf::from(file!());
        root.pop();
        root.pop();
        std::env::set_var(WORKSPACE, root);
    }
    if std::env::var(UPDATE).is_err() {
        std::env::set_var(UPDATE, "no");
    }
}

pub fn run(
    test_name: &str,
    is_cargo: bool,
    snapshot: &str,
    file: &str,
    module: &str,
    line: u32,
    expr: &str,
) {
    let command = if is_cargo {
        "INSTA_UPDATE=1 cargo test ..."
    } else {
        "buck test ... -- --env INSTA_UPDATE=1"
    };
    println!(
        "{:=^80}\n",
        format!(" Run `{}` to update snapshots ", command)
    );
    let file_name = std::path::Path::new(file)
        .file_name()
        .and_then(|p| p.to_str())
        .unwrap();
    insta::_macro_support::assert_snapshot(
        test_name.into(),
        snapshot,
        "unused",
        // buck builds have a _unittest module suffix which cargo doesn't
        // this makes the snapshot location consistent on both
        &module.replacen("_unittest", "", 1),
        file_name,
        line,
        expr,
    )
    .unwrap();
}

/// Assert that the serde json representation of given expression matches the snapshot
/// stored on disk.
#[macro_export]
macro_rules! assert_json {
    ($value: expr, $test_name: ident) => {{
        $crate::setup();

        $crate::run(
            stringify!($test_name),
            option_env!("CARGO_MANIFEST_DIR").is_some(),
            &serde_json::to_string(&$value).unwrap(),
            file!(),
            module_path!(),
            line!(),
            stringify!($value),
        );
    }};
}
