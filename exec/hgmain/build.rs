use std::env;

fn main() {
    if let Some(lib_dirs) = env::var_os("LIB_DIRS") {
        for lib_dir in std::env::split_paths(&lib_dirs) {
            println!("cargo:rustc-link-search={}", lib_dir.display());
        }
    }
}
