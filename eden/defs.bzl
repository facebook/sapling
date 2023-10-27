# This file contains macros that are shared across Eden.

load("@fbsource//tools/build_defs:buckconfig.bzl", "read_bool")

def get_oss_suffix():
    """Build rule suffix to use for open-source-specific build targets."""
    return "-oss"

def get_daemon_versions():
    """
    List of configurations to aid in creating dual build rules.

    Returns:
        An array of tuples where the first member is a build target for the
        daemon and the second member is the suffix to use for other templated
        build target names.
    """
    return [
        ("//eden/fs/service:edenfs%s" % suffix, suffix)
        for suffix in ["", get_oss_suffix()]
    ]

def get_dev_env_to_target(suffix = ""):
    """
    Returns a dict of environment variable names to binary targets, whose
    output locations the environment variables should specify in dev and
    integration test environments.
    """

    env_to_target = {
        "EDENFS_TRACE_STREAM": "//eden/fs/cli/trace:trace_stream",
    }
    if read_bool("fbcode", "mode_win_enabled", False):
        suffix = get_oss_suffix()

    env_to_target["EDENFS_SERVER_PATH"] = "//eden/fs/service:edenfs{}".format(suffix)
    return env_to_target

def get_dev_edenfsctl_env(additional_env = dict(), suffix = ""):
    env = {var: "$(location {})".format(target) for var, target in get_dev_env_to_target(suffix).items()}
    env.update(additional_env)
    return env

def get_test_env_and_deps(suffix = ""):
    """
    Returns env vars and a dep list that is useful for locating various
    build products from inside our tests
    """

    env_to_target = get_dev_env_to_target(suffix)

    if read_bool("fbcode", "mode_mac_enabled", False):
        env_to_target.update({
            "EDENFS_DROP_PRIVS": "//eden/fs/privhelper/test:drop_privs",
            "EDENFS_FSATTR_BIN": "//eden/integration/helpers:fsattr",
            "EDENFS_TAKEOVER_TOOL": "//eden/integration/helpers:takeover_tool",
            "EDEN_HG_BINARY": "//scm/telemetry/hg:hg",
            "HG_REAL_BIN": "//eden/scm:hg_universal_binary",
        })
    elif read_bool("fbcode", "mode_win_enabled", False):
        suffix = get_oss_suffix()
        env_to_target.update({
            "EDENFS_CHECK_WINDOWS_RENAME": "//eden/integration/helpers:check_windows_rename",
            "EDENFS_READ_REPARSE_BUFFER": "//eden/integration/helpers:read_reparse_buffer",
            "EDEN_HG_BINARY": "//eden/scm:hg",
            "HG_REAL_BIN": "//eden/scm:hg",
        })
    else:
        env_to_target.update({
            "EDENFS_DROP_PRIVS": "//eden/fs/privhelper/test:drop_privs",
            "EDENFS_FSATTR_BIN": "//eden/integration/helpers:fsattr",
            "EDENFS_TAKEOVER_TOOL": "//eden/integration/helpers:takeover_tool",
            "EDEN_HG_BINARY": "//scm/telemetry/hg:hg",
            "HG_REAL_BIN": "//eden/scm:hg",
        })

    daemon_target = "//eden/fs/service:edenfs%s" % suffix
    env_to_target.update({
        "BLAKE3_SUM": "//eden/integration/helpers:blake3_sum",
        "EDENFSCTL_REAL_PATH": "//eden/fs/cli:edenfsctl",
        "EDENFSCTL_RUST_PATH": "//eden/fs/cli_rs/edenfsctl:edenfsctl",
        "EDENFS_FAKE_EDENFS": "//eden/integration/helpers:fake_edenfs",
        "EDENFS_SNAPSHOTS": "//eden/test-data:snapshots",
        "EDENFS_ZERO_BLOB": "//eden/integration/helpers:zero_blob",
        "HG_ETC_MERCURIAL": "//eden/scm:etc_mercurial",
        "MKSCRATCH_BIN": "//eden/scm/exec/scratch:scratch",
    })

    envs = {
        "CHGDISABLE": "1",
        "EDENFS_SUFFIX": suffix,
    }
    deps = []

    for name, dep in sorted(env_to_target.items()):
        envs[name] = "$(location %s)" % dep
        deps.append(dep)

    # This one needs to be $(exe_target) since it's a command_alias.
    edenfsctl = "//eden/fs/cli_rs:edenfsctl-run{}".format(suffix)
    envs["EDENFS_CLI_PATH"] = "$(exe_target %s)" % edenfsctl
    deps.append(edenfsctl)

    return {
        "deps": deps,
        "env": envs,
    }

def get_integration_test_env_and_deps():
    """
    Returns env vars and a dep list for running integration tests.

    Intentionally uses the OSS build to limit dependencies and reduce build
    and run times.
    """
    return get_test_env_and_deps(get_oss_suffix())

def eden_is_compatible(compatible_with = None):
    # TODO(xavierd): Some rules don't support compatible_with, let's hack around it for now.
    if compatible_with:
        if read_bool("fbcode", "mode_win_enabled", False) and "ovr_config//os:windows" not in compatible_with:
            return False
        if read_bool("fbcode", "mode_mac_enabled", False) and "ovr_config//os:macos" not in compatible_with:
            return False
    return True

def make_rule_compatible_with(rule, compatible_with = None, **kwargs):
    if not eden_is_compatible(compatible_with):
        return

    rule(**kwargs)
