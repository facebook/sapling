load("@fbcode_macros//build_defs:native_rules.bzl", "buck_filegroup")
load("@fbsource//tools/build_defs/buck2:is_buck2.bzl", "is_buck2")
load(
    "//eden/mononoke/tests/integration/facebook:symlink_impl.bzl?v2_only",
    "symlink_v2",
)

symlink = symlink_v2 if is_buck2() else buck_filegroup
