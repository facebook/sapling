load("//fbpkg:fbpkg.bzl", "fbpkg")

LOG_PATHS = (
    "eden/mononoke",
    "common/rust",
    "third-party2/rust",
    "third-party2/rust-bindgen",
    "third-party2/rust-crates-io",
)

def mononoke_fbpkg(
        name,
        path_actions = None,
        override_log_paths = LOG_PATHS,
        compress_type = "squashfs",
        with_debug_symbols = True):
    """ Helper for defining Mononoke FBPKGs

        To each package it adds the mononoke config target, sets the right
        build config and compression. It also overrides the log_paths field
        to include all of Mononoke diffs.
    """
    if not path_actions:
        fail(
            'Empty `path_actions` found for "{}". '.format(name) +
            "You must specify this field so that we know what to package.",
        )

    buck_opts = fbpkg.buck_opts(
        mode = "opt",
    )
    if with_debug_symbols:
        buck_opts["config"].update({
            "fbcode.dwp": "true",
        })
    else:
        buck_opts["config"].update({
            "fbcode.build_dwp_targets": "false",
            "fbcode.dwp": "false",
            "fbcode.package_dwp_targets": "false",
        })

    path_actions = dict(path_actions)

    return fbpkg.builder(
        name = name,
        buck_opts = buck_opts,
        compress_type = compress_type,
        override_log_paths = list(override_log_paths),
        path_actions = path_actions,
        strip_debuginfo_without_saving_symbols = False if with_debug_symbols else True,
    )
