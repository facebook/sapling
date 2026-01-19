# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Build targets for EdenFS packaging.
# Maps build targets to their installation paths from packman.yml

# Main EdenFS build targets used in packaging
# Maps build target -> install path(s)

load("@fbcode//fbpkg:fbpkg.bzl", "fbpkg")
load("@fbcode//registry:defs.bzl", "rpm")

EDENFS_TARGETS = {
    "//eden/fs/cli/trace:trace_stream": "/usr/local/libexec/eden/eden_trace_stream",
    "//eden/fs/cli:edenfsctl": "/usr/local/bin/edenfsctl.real",
    "//eden/fs/cli_rs/edenfsctl:edenfsctl": "/usr/local/bin/edenfsctl",
    "//eden/fs/config/facebook:edenfs_config_manager": "/usr/local/libexec/eden/edenfs_config_manager",
    "//eden/fs/facebook:eden-fb303-collector": "/usr/local/libexec/eden/eden-fb303-collector",
    "//eden/fs/facebook:edenfs_restarter": "/usr/local/libexec/eden/edenfs_restarter",
    "//eden/fs/inodes/fscatalog:eden_fsck": "/usr/local/libexec/eden/eden_fsck",
    "//eden/fs/monitor:edenfs_monitor": "/usr/local/libexec/eden/edenfs_monitor",
    "//eden/fs/service:edenfs": "/usr/local/libexec/eden/edenfs",
    "//eden/fs/service:edenfs_privhelper": "/usr/local/libexec/eden/edenfs_privhelper",
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

MAC_TARGETS = {
    "//eden/scm/exec/eden_apfs_mount_helper:eden_apfs_mount_helper": "/usr/local/libexec/eden/eden_apfs_mount_helper",
}

def make_rpm_features():
    features = []
    for target, install_path in EDENFS_TARGETS.items():
        if target in TARGET_MODES:
            features.append(rpm.install(src = "fbcode" + target, dst = install_path, mode = TARGET_MODES.get(target)))
        else:
            features.append(rpm.install(src = "fbcode" + target, dst = install_path))
        if install_path in SYMLINKS:
            features.append(rpm.file_symlink(link = SYMLINKS.get(install_path), target = install_path))
    for dir in DIRS:
        features.append(rpm.ensure_dirs_exist(dir))
    for target, install_path in STATIC_TARGETS.items():
        if target in TARGET_MODES:
            features.append(rpm.install(src = target, dst = install_path, mode = TARGET_MODES.get(target)))
        else:
            features.append(rpm.install(src = target, dst = install_path))
    for target, install_path in SCRIPTS_TARGETS.items():
        features.append(rpm.install(src = target, dst = install_path, mode = 0o0755))
    for target, install_path in CONFIG_D_TARGETS.items():
        features.append(rpm.install(src = target, dst = install_path, mode = 0o0755))

    mac_features = []

    for target, install_path in MAC_TARGETS.items():
        if target in TARGET_MODES:
            mac_features.append(rpm.install(src = "fbcode" + target, dst = install_path, mode = TARGET_MODES.get(target)))
        else:
            mac_features.append(rpm.install(src = "fbcode" + target, dst = install_path))
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
    path_actions["etc/eden/config.d"] = "fs/facebook/packaging/config.d"

    # Misc fbpkg files
    # static file for DevFeature installation instructions
    path_actions["install.toml"] = "facebook/dev_feature_install.toml"

    # static file for eden-locale
    path_actions["locale/en/LC_MESSAGES/libc.mo"] = "locale/glibc_en.mo"

    # static file for Sandcastle live-installation
    path_actions["sandcastle_install.sh"] = "facebook/sandcastle_install.sh"

    return path_actions
