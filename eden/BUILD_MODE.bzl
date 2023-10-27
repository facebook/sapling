""" build mode definitions for eden """

load("@fbcode_macros//build_defs:create_build_mode.bzl", "create_build_mode")

COMPILE_TIME_TRACING = False

_extra_cxxflags = [
    "-Wall",
    "-Wextra",
    "-Wuninitialized",
    "-Wtype-limits",
    "-Werror=return-type",
]

_extra_clang_flags = [
    "-Wconstant-conversion",
    "-Wgnu-conditional-omitted-operand",
    "-Wheader-hygiene",
    "-Wimplicit-fallthrough",
    "-Wshadow",
    "-Wshift-sign-overflow",
    "-Wunused-const-variable",
    "-Wunused-exception-parameter",
    "-Wunused-lambda-capture",
    "-Wunused-value",
    "-Wunused-variable",
    "-Wunreachable-code-aggressive",
    "-Wno-nullability-completeness",
    "-Winconsistent-missing-override",
] + (["-ftime-trace"] if COMPILE_TIME_TRACING else [])

_extra_gcc_flags = [
    "-Wunused-but-set-variable",
    "-Wshadow",
]

_os_preprocessor_flags = [
    ("windows", [
        # Note: as of 2023, libevent undefines WIN32_LEAN_AND_MEAN after
        # including <windows.h>. This can be confusing, but it should be
        # okay. If libevent includes <windows.h>, then later includes of
        # windows.h should not pull in Winsock 1.
        "-DWIN32_LEAN_AND_MEAN",
        "-DNOMINMAX",
        "-DSTRICT",
    ]),
]

_mode = create_build_mode(
    clang_flags = _extra_clang_flags,
    cxx_flags = _extra_cxxflags,
    gcc_flags = _extra_gcc_flags,
    os_preprocessor_flags = _os_preprocessor_flags,
)

_modes = {
    "dbg": _mode,
    "dbgo": _mode,
    "dev": _mode,
    "opt": _mode,
}

def get_modes():
    """ Return modes for this hierarchy """
    return _modes
