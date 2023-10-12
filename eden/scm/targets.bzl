load("@fbcode_macros//build_defs:rust_library.bzl", "rust_library")

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
