extern crate cc;

use std::env;

fn main() {
    let target = env::var("TARGET").unwrap();

    let mut build = cc::Build::new();

    build
        .include("webview.h")
        .flag_if_supported("-std=c11")
        .flag_if_supported("-w");

    if env::var("DEBUG").is_err() {
        build.define("NDEBUG", None);
    } else {
        build.define("DEBUG", None);
    }

    if target.contains("apple") {
        build
            .file("webview_cocoa.c")
            .flag("-x")
            .flag("objective-c");
        println!("cargo:rustc-link-lib=framework=Cocoa");
        println!("cargo:rustc-link-lib=framework=WebKit");
    } else {
        panic!("unsupported target (only macos is supported)");
    }

    build.compile("webview");
}
