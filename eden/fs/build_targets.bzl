# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Build targets for EdenFS packaging.
# Maps build targets to their installation paths in the packaged layout.

# Main EdenFS build targets used in packaging
# Maps build target -> install path(s)

load("@fbcode//fbpkg:fbpkg.bzl", "fbpkg")
load("@fbcode//registry:defs.bzl", "rpm")

EDENFS_TARGETS = {
    "//eden/fs/cli/trace:trace_stream": "/usr/local/libexec/eden/eden_trace_stream",
    "//eden/fs/cli:edenfsctl": "/usr/local/bin/edenfsctl.real",
    "//eden/fs/cli_rs/edenfsctl:edenfsctl": "/usr/local/bin/edenfsctl",
    "//eden/fs/config/facebook/config_manager_rs:edenfs_config_manager_rust": "/usr/local/libexec/eden/edenfs_config_manager_rust",
    "//eden/fs/config/facebook:edenfs_config_manager": "/usr/local/libexec/eden/edenfs_config_manager",
    "//eden/fs/facebook:eden-fb303-collector": "/usr/local/libexec/eden/eden-fb303-collector",
    "//eden/fs/facebook:edenfs_restarter": "/usr/local/libexec/eden/edenfs_restarter",
    "//eden/fs/inodes/fscatalog:eden_fsck": "/usr/local/libexec/eden/eden_fsck",
    "//eden/fs/monitor:edenfs_monitor": "/usr/local/libexec/eden/edenfs_monitor",
    "//eden/fs/service:edenfs": "/usr/local/libexec/eden/edenfs",
    "//eden/fs/service:edenfs_privhelper": "/usr/local/libexec/eden/edenfs_privhelper",
}

MAC_PYTHON_PAR_SHIM = "//eden/fs:mac_python_par_shim_universal"

# Mac replacements for targets that are also installed in Linux RPMs. Values
# are (mac_src, mac_mode). If mac_mode is None, the normal TARGET_MODES entry
# for the original target still applies.
MAC_TARGET_OVERRIDES = {
    "//eden/fs/cli/trace:trace_stream": ("//eden/fs:trace_stream_universal", None),
    "//eden/fs/cli:edenfsctl": (MAC_PYTHON_PAR_SHIM, 0o0755),
    "//eden/fs/cli_rs/edenfsctl:edenfsctl": ("//eden/fs:edenfsctl_universal", None),
    "//eden/fs/config/facebook/config_manager_rs:edenfs_config_manager_rust": ("//eden/fs:edenfs_config_manager_rust_universal", None),
    "//eden/fs/config/facebook:edenfs_config_manager": (MAC_PYTHON_PAR_SHIM, 0o0755),
    "//eden/fs/facebook:eden-fb303-collector": ("//eden/fs:eden-fb303-collector_universal", None),
    "//eden/fs/facebook:edenfs_restarter": (MAC_PYTHON_PAR_SHIM, 0o0755),
    "//eden/fs/inodes/fscatalog:eden_fsck": ("//eden/fs:eden_fsck_universal", None),
    "//eden/fs/monitor:edenfs_monitor": ("//eden/fs:edenfs_monitor_universal", None),
    "//eden/fs/service:edenfs": ("//eden/fs:edenfs_universal", None),
    "//eden/fs/service:edenfs_privhelper": ("//eden/fs:edenfs_privhelper_universal", None),
}

SYMLINKS = {
    "/usr/local/bin/edenfsctl": "/usr/local/bin/eden",
}

STATIC_TARGETS = {
    "facebook/packaging/NOT_MOUNTED_README.txt": "/etc/eden/NOT_MOUNTED_README.txt",
    "facebook/packaging/ignore": "/etc/eden/ignore",
}

SCRIPTS_TARGETS = {
    "scripts/facebook/eden_bench.sh": "/usr/local/libexec/eden/eden_bench.sh",
    "scripts/facebook/eden_prof": "/usr/local/libexec/eden/eden_prof",
    "scripts/facebook/rg_perf_test": "/usr/local/libexec/eden/eden_rg_perf_script",
}

CONFIG_D_TARGETS = {
    "facebook/packaging/config.d/00-defaults.toml": "/etc/eden/config.d/00-defaults.toml",
    "facebook/packaging/config.d/doctor.toml": "/etc/eden/config.d/doctor.toml",
}

EDENFS_WINDOWS_DEPS = [
    "fbcode//eden/fs/cli:edenfsctl",
    "fbcode//eden/fs/cli/trace:trace_stream",
    "fbcode//eden/fs/cli_rs/edenfsctl:edenfsctl",
    "fbcode//eden/fs/config/facebook:edenfs_config_manager",
    "fbcode//eden/fs/config/facebook/config_manager_rs:edenfs_config_manager_rust",
    "fbcode//eden/fs/facebook:eden-fb303-collector",
    "fbcode//eden/fs/service:edenfs",
    "fbcode//eden/fs/service:edenfs[pdb]",
    # TODO: Figure out symbol package "//arvr/tools/translator:symbol_ents"
    # TODO: Set up install script to copy files to the appropriate locations
]

FBPKG_STATIC_ADD_PREFIX = "fs/"
FBPKG_STRIP_PREFIX = "/usr/local/"

TARGET_MODES = {
    "//eden/fs/service:edenfs_privhelper": 0o04755,
    "//eden/scm/exec/eden_apfs_mount_helper:eden_apfs_mount_helper": 0o04755,
    "facebook/packaging/ignore": 0o0755,
}

DIRS = [
    "/etc/eden",
    "/etc/eden/config.d",
]

SYSTEMD_STATIC_TARGETS = {
    "facebook/packaging/systemd/edenfs.slice": "/usr/lib/systemd/user/edenfs.slice",
    "facebook/packaging/systemd/edenfs@.service": "/usr/lib/systemd/user/edenfs@.service",
}

MAC_ONLY_TARGETS = {
    "//eden/fs:eden_apfs_mount_helper_universal": ("/usr/local/libexec/eden/eden_apfs_mount_helper", 0o04755),
    "//eden/fs:edenfs_config_manager_macos_arm64": ("/usr/local/libexec/eden/edenfs_config_manager.arm64", None),
    "//eden/fs:edenfs_config_manager_macos_x86_64": ("/usr/local/libexec/eden/edenfs_config_manager.x86_64", None),
    "//eden/fs:edenfs_restarter_macos_arm64": ("/usr/local/libexec/eden/edenfs_restarter.arm64", None),
    "//eden/fs:edenfs_restarter_macos_x86_64": ("/usr/local/libexec/eden/edenfs_restarter.x86_64", None),
    "//eden/fs:edenfsctl_real_macos_arm64": ("/usr/local/bin/edenfsctl.real.arm64", None),
    "//eden/fs:edenfsctl_real_macos_x86_64": ("/usr/local/bin/edenfsctl.real.x86_64", None),
}

def _rpm_install(src, dst, mode = None):
    if mode != None:
        return rpm.install(src = src, dst = dst, mode = mode)
    return rpm.install(src = src, dst = dst)

def make_rpm_features():
    features = []
    for target, install_path in EDENFS_TARGETS.items():
        default_feature = _rpm_install(
            src = "fbcode" + target,
            dst = install_path,
            mode = TARGET_MODES.get(target),
        )
        mac_override = MAC_TARGET_OVERRIDES.get(target)
        if mac_override:
            mac_src, mac_mode = mac_override
            features.append(select({
                "DEFAULT": default_feature,
                "ovr_config//os:macos": _rpm_install(
                    src = "fbcode" + mac_src,
                    dst = install_path,
                    mode = mac_mode or TARGET_MODES.get(target),
                ),
            }))
        else:
            features.append(default_feature)
        if install_path in SYMLINKS:
            features.append(rpm.file_symlink(link = SYMLINKS.get(install_path), target = install_path))
    for dir in DIRS:
        features.append(rpm.ensure_dirs_exist(dir))
    for target, install_path in STATIC_TARGETS.items():
        if target in TARGET_MODES:
            features.append(rpm.install(src = target, dst = install_path, mode = TARGET_MODES.get(target)))
        else:
            features.append(rpm.install(src = target, dst = install_path))
    for target, install_path in SYSTEMD_STATIC_TARGETS.items():
        features.append(
            select({
                "DEFAULT": rpm.install(src = target, dst = install_path),
                # No Systemd on mac
                "ovr_config//os:macos": None,
            }),
        )
    for target, install_path in SCRIPTS_TARGETS.items():
        features.append(rpm.install(src = target, dst = install_path, mode = 0o0755))
    for target, install_path in CONFIG_D_TARGETS.items():
        features.append(rpm.install(src = target, dst = install_path, mode = 0o0755))

    mac_features = []

    for target, (install_path, mode) in MAC_ONLY_TARGETS.items():
        mac_features.append(_rpm_install(src = "fbcode" + target, dst = install_path, mode = mode))
    for mac_feature in mac_features:
        features.append(
            select({
                "DEFAULT": None,
                "ovr_config//os:macos": mac_feature,
            }),
        )
    return features

def make_fbpkg_path_actions():
    path_actions = {}
    for target, install_path in EDENFS_TARGETS.items():
        path_actions[install_path.removeprefix(FBPKG_STRIP_PREFIX)] = fbpkg.copy(target)
        if install_path in SYMLINKS:
            path_actions[SYMLINKS.get(install_path).removeprefix(FBPKG_STRIP_PREFIX)] = fbpkg.symlink(install_path.removeprefix(FBPKG_STRIP_PREFIX))

    for target, install_path in SCRIPTS_TARGETS.items():
        path_actions[install_path.removeprefix(FBPKG_STRIP_PREFIX)] = FBPKG_STATIC_ADD_PREFIX + target

    for target, install_path in STATIC_TARGETS.items():
        path_actions[install_path.removeprefix("/")] = FBPKG_STATIC_ADD_PREFIX + target

    # Currently no plans to install systemd targets on sandcastle

    path_actions["etc/eden/config.d"] = "fs/facebook/packaging/config.d"

    # Misc fbpkg files
    # static file for DevFeature installation instructions
    path_actions["install.toml"] = "facebook/dev_feature_install.toml"

    # static file for eden-locale
    path_actions["locale/en/LC_MESSAGES/libc.mo"] = "locale/glibc_en.mo"

    # static file for Sandcastle live-installation
    path_actions["sandcastle_install.sh"] = "facebook/sandcastle_install.sh"

    return path_actions
