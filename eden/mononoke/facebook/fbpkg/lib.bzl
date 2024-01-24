load("//antlir/fbpkg:fbpkg.bzl", "fbpkg")

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
        compress_type = "squashfs"):
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

    path_actions = dict(path_actions)
    path_actions["config"] = "//eden/mononoke/facebook/config:config"

    fbpkg.builder(
        name = name,
        buck_opts = fbpkg.buck_opts(
            config = {
                "fbcode.dwp": "true",
            },
            mode = "opt",
            version = "v2",
        ),
        compress_type = compress_type,
        fail_on_redundant_configerator_fbpkg = False,
        override_log_paths = list(override_log_paths),
        path_actions = path_actions,
    )
