load("@fbcode_macros//build_defs:native_rules.bzl", "buck_genrule", "buck_sh_binary")
load("@fbcode_macros//build_defs:rust_library.bzl", "rust_library")
load("@fbsource//tools/build_defs:buckconfig.bzl", "read_bool")

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

def gen_hgpython():
    if read_bool("fbcode", "mode_win_enabled", False) and "ovr_config//os:windows":
        return buck_genrule(
            name = "hgpython",
            out = "python.exe",
            bash = "ln -s $(location :hg) $OUT",
            cmd_exe = "mklink $OUT $(location :hg)",
            executable = True,
        )

    # We cannot quite use symlinks outside of Windows since the `dev-nosan-lg` mode is
    # used sometimes, and that copies the binary into another location rather
    # than actually creating a symlink like in other modes for some reason.
    return buck_sh_binary(
        name = "hgpython",
        main = "run_buck_hgpython.sh",
        resources = [
            "fbcode//eden/scm:hg",
        ],
    )
