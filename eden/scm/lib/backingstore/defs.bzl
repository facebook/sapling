load("@fbcode_macros//build_defs:cpp_library.bzl", "cpp_library")
load("@fbcode_macros//build_defs:rust_library.bzl", "rust_library")

def backing_store(name, features = [], extra_deps = [], **kwargs):
    cpp_library(
        name = "sapling_native_%s" % name,
        srcs = glob(["src/**/*.cpp"]),
        headers = glob(["include/**/*.h"]),
        undefined_symbols = True,
        deps = [
            "//folly:string",
            "//folly/io:iobuf",
            "//folly/logging:logging",
        ],
        exported_deps = [
            "fbsource//third-party/rust:cxx-core",
            ":%s@header" % name,
            "//eden/fs/store:context",
            "//folly:function",
            "//folly:range",
            "//folly:try",
            "//folly/futures:core",
        ],
    )

    rust_library(
        name = name,
        srcs = glob(["src/**/*.rs"]),
        cpp_deps = [
            ":sapling_native_%s" % name,
            "//eden/fs/store:context",
        ],
        crate_root = "src/lib.rs",
        cxx_bridge = "src/ffi.rs",
        features = features,
        deps = [
            "fbsource//third-party/rust:anyhow",
            "fbsource//third-party/rust:arc-swap",
            "fbsource//third-party/rust:cxx",
            "fbsource//third-party/rust:env_logger",
            "fbsource//third-party/rust:log",
            "fbsource//third-party/rust:parking_lot",
            "fbsource//third-party/rust:tracing",
            "fbsource//third-party/rust:tracing-subscriber",
            "//eden/scm/lib/config/loader:configloader",
            "//eden/scm/lib/constructors:constructors",
            "//eden/scm/lib/eagerepo:eagerepo",
            "//eden/scm/lib/edenapi:edenapi",
            "//eden/scm/lib/identity:identity",
            "//eden/scm/lib/indexedlog:indexedlog",
            "//eden/scm/lib/manifest:manifest",
            "//eden/scm/lib/repo:repo",
            "//eden/scm/lib/storemodel:storemodel",
            "//eden/scm/lib/tracing-collector:tracing-collector",
            "//eden/scm/lib/types:types",
        ] + extra_deps,
        **kwargs
    )
