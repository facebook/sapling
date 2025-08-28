load("@fbsource//tools/build_defs:fb_xplat_rust_library.bzl", "fb_xplat_rust_library")
load("@fbsource//tools/build_defs:fbsource_utils.bzl", "is_xplat")
load("@fbsource//tools/build_defs:rust_library.bzl", _fbcode_rust_library = "rust_library")

def fbthrift_library(**kwargs):
    if is_xplat():
        kwargs.pop("autocargo", None)
        fb_xplat_rust_library(**kwargs)
    else:
        kwargs.pop("platforms", None)
        kwargs.pop("xplat_preexisting_target_flavors", None)
        _fbcode_rust_library(**kwargs)
