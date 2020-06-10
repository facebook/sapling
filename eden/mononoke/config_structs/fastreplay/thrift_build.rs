// @generated
use std::env;
use std::fs;
use std::path::Path;

use thrift_compiler::Config;

#[rustfmt::skip]
fn main() {
    let out_dir = env::var_os("OUT_DIR").expect("OUT_DIR env not provided");
    let out_dir: &Path = out_dir.as_ref();
    fs::write(
        out_dir.join("cratemap"),
        "fastreplay _ crate",
    ).expect("Failed to write cratemap");

    let conf = {
        let mut conf = Config::from_env().expect("Failed to instantiate thrift_compiler::Config");

        let path_from_manifest_to_base: &Path = "../../../../..".as_ref();
        let cargo_manifest_dir =
            env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not provided");
        let cargo_manifest_dir: &Path = cargo_manifest_dir.as_ref();
        let base_path = cargo_manifest_dir
            .join(path_from_manifest_to_base)
            .canonicalize()
            .expect("Failed to canonicalize base_path");
        conf.base_path(base_path);

        let options = "serde";
        if !options.is_empty() {
            conf.options(options);
        }

        conf
    };

    conf
        .run(&[
            "fastreplay.thrift"
        ])
        .expect("Failed while running thrift compilation");
}
