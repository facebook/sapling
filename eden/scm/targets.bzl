load("@fbcode_macros//build_defs:native_rules.bzl", "buck_genrule", "buck_sh_binary")
load("@fbcode_macros//build_defs/lib:rust_oss.bzl", "rust_oss")
load("@fbsource//tools/build_defs:buckconfig.bzl", "read_bool")
load("@fbsource//tools/build_defs:rust_binary.bzl", "rust_binary")
load("@fbsource//tools/build_defs:rust_library.bzl", "rust_library")
load("@fbsource//tools/target_determinator/macros:ci_hint.bzl", "ci_hint")

def _set_default(obj, *keys):
    for key in keys:
        obj[key] = obj.get(key) or {}
        obj = obj[key]
    return obj

def exec_compatible_with_target():
    """Intended to be used by genrule's exec_compatible_with to force
    execution platform to match target platform. Without this, one
    might get "exec format error" when target platform (ex. macos)
    binary is run on incompatible execution platform (ex. linux).

    This function provides a shorter way for code like:

        exec_compatible_with = select({
            "ovr_config//os:linux": ["ovr_config//os:linux"],
            "ovr_config//os:macos": ["ovr_config//os:macos"],
            ...
            "DEFAULT": [],
        }) + select({
            "ovr_config//cpu:x86_64": ["ovr_config//cpu:x86_64"],
            "ovr_config//cpu:arm64": ["ovr_config//cpu:arm64"],
            ...
            "DEFAULT": [],
        })
    """

    # To list low-level constraints (could be noisy), use:
    #     buck uquery "attrfilter(constraint_setting, 'ovr_config//os/constraints:os', deps('ovr_config//os/constraints:'))"
    #     buck uquery "attrfilter(constraint_setting, 'ovr_config//cpu/constraints:cpu', deps('ovr_config//cpu/constraints:'))"
    # To list high-level constraints (that might mix OS and CPU like "windows-arm64"), use:
    #     buck uquery "deps('ovr_config//os:', 0)"
    #     buck uquery "deps('ovr_config//cpu:', 0)"
    os_list = ["emscripten", "freebsd", "linux", "macos", "wasi", "windows"]
    cpu_list = ["arm", "arm32", "arm64", "riscv32", "riscv64", "wasm32", "x86_32", "x86_64", "xtensa"]

    def select_with(prefix, names):
        choices = {"DEFAULT": []}
        for name in names:
            full_name = prefix + name
            choices[full_name] = [full_name]
        return select(choices)

    return select_with("ovr_config//os/constraints:", os_list) + select_with("ovr_config//cpu/constraints:", cpu_list)

def rust_python_library(deps = None, include_python_sys = False, include_cpython = True, **kwargs):
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
        cpython = _set_default(kwargs3, "autocargo", "cargo_toml_config", "dependencies_override", "dependencies", "cpython")
        cpython["features"] = cpython.get("features") or ["python3-sys"]
    if include_python_sys:
        deps3.append("fbsource//third-party/rust:python3-sys")

    kwargs3["name"] = kwargs["name"]
    kwargs3["crate"] = kwargs["name"].replace("-", "_")
    kwargs3["deps"] = deps3
    rust_library(**kwargs3)

def gen_hgpython(hg_target, suffix = ""):
    if read_bool("fbcode", "mode_win_enabled", False) and "ovr_config//os:windows":
        return buck_genrule(
            name = "hgpython" + suffix,
            out = "python.exe",
            bash = "ln -s $(location " + hg_target + ") $OUT",
            cmd_exe = "mklink $OUT $(location :hg)",
            executable = True,
        )

    # We cannot quite use symlinks outside of Windows since the `dev-nosan-lg` mode is
    # used sometimes, and that copies the binary into another location rather
    # than actually creating a symlink like in other modes for some reason.
    return buck_sh_binary(
        name = "hgpython" + suffix,
        main = "run_buck_hgpython.sh",
        resources = [
            hg_target,
        ],
    )

def is_experimental_cas_build():
    return read_bool("sl", "cas", False)

def fetch_as_eden():
    return read_bool("sl", "fetch_as_eden", False)

def hg_binary(name, extra_deps = [], extra_features = [], **kwargs):
    rust_binary(
        name = name,
        srcs = glob(["exec/hgmain/src/**/*.rs"]),
        features = [
            "fb",
            "with_chg",
        ] + extra_features,
        link_style = "static",
        linker_flags = select({
            "DEFAULT": [],
            "ovr_config//os:windows": [
                "/MANIFEST:EMBED",
                "/MANIFESTINPUT:$(location :windows-manifest)",
            ],
        }),
        # hgmain should be buildable by stable Rust (via cargo). Disallow nightly Rust features.
        rustc_flags = ["-Zallow-features="],
        os_deps = [
            (
                "linux",
                [
                    "fbsource//third-party/rust:dirs",
                    "fbsource//third-party/rust:libc",
                    ":chg",
                    "//eden/scm/lib/config/loader:configloader",
                    "//eden/scm/lib/config/model:configmodel",
                    "//eden/scm/lib/encoding:encoding",
                    "//eden/scm/lib/identity:identity",
                    "//eden/scm/lib/version:rust_version",
                ],
            ),
            (
                "macos",
                [
                    "fbsource//third-party/rust:dirs",
                    "fbsource//third-party/rust:libc",
                    ":chg",
                    "//eden/scm/lib/config/loader:configloader",
                    "//eden/scm/lib/config/model:configmodel",
                    "//eden/scm/lib/encoding:encoding",
                    "//eden/scm/lib/identity:identity",
                    "//eden/scm/lib/version:rust_version",
                    "//eden/scm/lib/webview-app:webview-app",
                ],
            ),
            (
                "windows",
                [
                    "fbsource//third-party/rust:anyhow",
                    "fbsource//third-party/rust:winapi",
                ],
            ),
        ],
        deps = [
            "fbsource//third-party/rust:tracing",
            "//eden/scm/lib/clidispatch:clidispatch",
            "//eden/scm/lib/commands:commands",
            "//eden/scm/lib/util/atexit:atexit",
        ] + extra_deps + ([] if rust_oss.is_oss_build() else [
            "//common/rust/shed/fbinit:fbinit",
            "//common/rust/cpp_log_spew:cpp_log_spew",
        ]),
        **kwargs
    )

    # Try to override target depth so //eden/scm/tests:hg_run_tests and other
    # important test targets reliably pick up Python code changes despite target
    # depth greater than 4.
    ci_hint(
        ci_deps = ["fbcode//eden/scm/lib/python-modules:python-modules"],
        reason = "hg is very close to Python source files despite large target depth",
        target = name,
    )
