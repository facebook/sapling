load("@fbcode_macros//build_defs:native_rules.bzl", "alias")
load("@fbcode_macros//build_defs:rust_binary.bzl", "rust_binary")
load("@fbcode_macros//build_defs:rust_library.bzl", "rust_library")
load("@fbcode_macros//build_defs:rust_universal_binary.bzl", "rust_universal_binary")
load("@fbsource//tools/build_defs:buckconfig.bzl", "read_bool")

def rust_maybe_universal_binary(name, **kwargs):
    rust_name = "rust_" + name
    rust_binary(
        name = rust_name,
        **kwargs
    )
    if read_bool("fbcode", "mode_mac_enabled", False):
        rust_universal_binary(
            name = name,
            source = ":{}".format(rust_name),
            visibility = kwargs.get("visibility", None),
        )
    else:
        alias(
            name = name,
            actual = ":{}".format(rust_name),
            visibility = kwargs.get("visibility", None),
        )

def rust_python_library(deps = None, include_python_sys = False, include_cpython = True, **kwargs):
    if "versions" not in kwargs:
        kwargs["versions"] = {}

    # Python 3 target
    kwargs3 = dict(kwargs)

    deps3 = list(deps or [])
    if include_cpython:
        deps3.append("fbsource//third-party/rust:cpython")
        asan_opts = kwargs3.setdefault("test_env", {}).setdefault("ASAN_OPTIONS", "")
        if asan_opts:
            kwargs3["test_env"]["ASAN_OPTIONS"] += ":use_sigaltstack=0"
        else:
            kwargs3["test_env"]["ASAN_OPTIONS"] = "use_sigaltstack=0"
    if include_python_sys:
        deps3.append("fbsource//third-party/rust:python3-sys")

    kwargs3["name"] = kwargs["name"]
    kwargs3["crate"] = kwargs["name"].replace("-", "_")
    kwargs3["deps"] = deps3
    kwargs3["versions"]["python"] = "3.10"
    rust_library(**kwargs3)
