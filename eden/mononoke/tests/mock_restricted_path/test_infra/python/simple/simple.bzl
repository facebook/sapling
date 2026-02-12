load("@fbcode_macros//build_defs:python_library.bzl", "python_library")
load("@fbcode_macros//build_defs:python_unittest.bzl", "python_unittest")

def py_tests_and_libs(
        name,
        srcs,
        test_srcs):
    par_styles = [
        "zip",
        "live",
        "fastzip",
        "xar",
    ]

    for base_module_mapping in [
        None,
        "base_module_mapped",
    ]:
        library_name = name
        if base_module_mapping != None:
            library_name += "_" + base_module_mapping

        python_library(
            name = library_name,
            srcs = srcs,
            base_module = base_module_mapping,
        )

        for par_style in par_styles:
            test_name = "simple_test"
            if base_module_mapping != None:
                test_name += "_" + base_module_mapping
            test_name += "_par_style_" + par_style

            python_unittest(
                name = test_name,
                srcs = test_srcs,
                base_module = base_module_mapping,
                needed_coverage = [(50, ":" + library_name)],
                par_style = par_style,
                deps = [":" + library_name],
            )
