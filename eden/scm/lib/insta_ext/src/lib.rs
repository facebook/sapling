/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// for macros
pub use insta;

pub fn setup() {
    const WORKSPACE: &str = "INSTA_WORKSPACE_ROOT";
    const UPDATE: &str = "INSTA_UPDATE";
    if std::env::var(WORKSPACE).is_err() {
        let mut root = std::path::PathBuf::from(file!());
        assert!(root.pop());
        assert!(root.pop());
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var(WORKSPACE, root) };
    }
    if std::env::var(UPDATE).is_err() {
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var(UPDATE, "no") };
    }
}

pub fn run(
    test_name: &str,
    is_cargo: bool,
    snapshot: &str,
    file: &str,
    function_name: &str,
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
        function_name,
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
            $crate::insta::_function_name!(),
            module_path!(),
            line!(),
            stringify!($value),
        );
    }};
}
