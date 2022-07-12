#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import binascii
import collections
import datetime
import errno
import json
import logging
import os
import shutil
import struct
import subprocess
import sys
import time
import typing
import uuid
from pathlib import Path
from typing import Any, Dict, IO, KeysView, List, Mapping, Optional, Set, Tuple, Union

import facebook.eden.ttypes as eden_ttypes
import toml
from eden.thrift import legacy
from eden.thrift.legacy import EdenNotRunningError
from facebook.eden.ttypes import MountInfo as ThriftMountInfo, MountState
from filelock import BaseFileLock, FileLock

from . import configinterpolator, configutil, telemetry, util, version
from .util import HealthStatus, print_stderr, write_file_atomically


try:
    from eden.thrift import client  # @manual
except ImportError:
    # Thrift-py3 is not supported in the CMake build yet.
    pass


log: logging.Logger = logging.getLogger(__name__)

# On Linux we import fcntl for flock. The Windows LockFileEx is not semantically
# same as flock. We will need to make some changes for LockFileEx to work.
if sys.platform != "win32":
    import fcntl


if typing.TYPE_CHECKING:
    from eden.fs.cli.redirect import RedirectionType  # noqa: F401

# Use --etcEdenDir to change the value used for a given invocation
# of the eden cli.
if sys.platform == "win32":
    DEFAULT_ETC_EDEN_DIR = "C:\\ProgramData\\facebook\\eden"
else:
    DEFAULT_ETC_EDEN_DIR = "/etc/eden"

# These are INI files that hold config data.
# CONFIG_DOT_D is relative to DEFAULT_ETC_EDEN_DIR, or whatever the
# effective value is for that path
CONFIG_DOT_D = "config.d"
# USER_CONFIG is relative to the HOME dir for the user
USER_CONFIG = ".edenrc"
# SYSTEM_CONFIG is relative to the etc eden dir
SYSTEM_CONFIG = "edenfs.rc"

# These paths are relative to the user's client directory.
CLIENTS_DIR = "clients"
CONFIG_JSON = "config.json"
CONFIG_JSON_LOCK = "config.json.lock"

# These are files in a client directory.
CLONE_SUCCEEDED = "clone-succeeded"
MOUNT_CONFIG = "config.toml"
SNAPSHOT = "SNAPSHOT"
SNAPSHOT_MAGIC_1 = b"eden\x00\x00\x00\x01"
SNAPSHOT_MAGIC_2 = b"eden\x00\x00\x00\x02"
SNAPSHOT_MAGIC_3 = b"eden\x00\x00\x00\x03"
SNAPSHOT_MAGIC_4 = b"eden\x00\x00\x00\x04"

DEFAULT_REVISION = {  # supported repo name -> default bookmark
    "git": "refs/heads/master",
    "hg": "first(present(master) + .)",
    "recas": "",
}

SUPPORTED_REPOS: KeysView[str] = DEFAULT_REVISION.keys()

SUPPORTED_MOUNT_PROTOCOLS = {"fuse", "nfs", "prjfs"}

# Create a readme file with this name in the mount point directory.
# The intention is for this to contain instructions telling users what to do if their
# EdenFS mount is not currently mounted.
NOT_MOUNTED_README_PATH = "README_EDEN.txt"
# The path under /etc/eden where site-specific contents for the not-mounted README can
# be found.
NOT_MOUNTED_SITE_SPECIFIC_README_PATH = "NOT_MOUNTED_README.txt"
# The default contents for the not-mounted README if a site-specific template
# is not found.
NOT_MOUNTED_DEFAULT_TEXT = """\
This directory is the mount point for a virtual checkout managed by EdenFS.

If you are seeing this file that means that your repository checkout is not
currently mounted.  This could either be because the edenfs daemon is not
currently running, or it simply does not have this checkout mounted yet.

You can run "eden doctor" to check for problems with EdenFS and try to have it
automatically remount your checkouts.
"""


class UsageError(Exception):
    pass


class InProgressCheckoutError(Exception):
    from_commit: str
    to_commit: str
    pid: int

    def __init__(self, from_commit: str, to_commit: str, pid: int) -> None:
        super().__init__()
        self.from_commit = from_commit
        self.to_commit = to_commit
        self.pid = pid

    def __str__(self) -> str:
        return (
            f"A checkout operation is ongoing: {self.from_commit} -> {self.to_commit}"
        )


class CheckoutConfig(typing.NamedTuple):
    """Configuration for an EdenFS checkout. A checkout stores its config in config.toml
    it its state directory (.eden/clients/<checkout_name>/config.toml)

    - backing_repo: The path where the true repo resides on disk.  For mercurial backing
        repositories this does not include the final ".hg" directory component.
    - scm_type: "hg" or "git"
    - mount_protocol: "fuse", "nfs" or "prjfs"
    - case_sensitive: whether the mount point is case sensitive. Default to
      false on Windows and macOS.
    - require_utf8_path: whether the mount point will disallow non-utf8 paths
      to be written to it.
    - guid: Used on Windows by ProjectedFS to identify this checkout.
    - redirections: dict where keys are relative pathnames in the EdenFS mount
      and the values are RedirectionType enum values that describe the type of
      the redirection.
    """

    backing_repo: Path
    scm_type: str
    guid: str
    mount_protocol: str
    case_sensitive: bool
    require_utf8_path: bool
    default_revision: str
    redirections: Dict[str, "RedirectionType"]
    active_prefetch_profiles: List[str]
    predictive_prefetch_profiles_active: bool
    predictive_prefetch_num_dirs: int
    enable_tree_overlay: bool
    use_write_back_cache: bool


class ListMountInfo(typing.NamedTuple):
    path: Path
    data_dir: Path
    state: Optional[MountState]
    configured: bool
    backing_repo: Optional[Path]

    def to_json_dict(self) -> Dict[str, Any]:
        return {
            "data_dir": self.data_dir.as_posix(),
            "state": MountState._VALUES_TO_NAMES.get(self.state)
            if self.state is not None
            else "NOT_RUNNING",
            "configured": self.configured,
            "backing_repo": self.backing_repo.as_posix()
            if self.backing_repo is not None
            else None,
        }


class SnapshotState(typing.NamedTuple):
    working_copy_parent: str
    last_checkout_hash: str


class EdenInstance:
    """This class contains information about a particular edenfs instance.

    It provides APIs for communicating with edenfs over thrift and for examining and
    modifying the list of checkouts managed by this edenfs instance.
    """

    _telemetry_logger: Optional[telemetry.TelemetryLogger] = None
    _home_dir: Path
    _user_config_path: Path
    _system_config_path: Path
    _config_dir: Path

    def __init__(
        self,
        config_dir: Union[Path, str, None],
        etc_eden_dir: Union[Path, str, None],
        home_dir: Union[Path, str, None],
        interpolate_dict: Optional[Dict[str, str]] = None,
    ) -> None:
        self._etc_eden_dir = Path(etc_eden_dir or DEFAULT_ETC_EDEN_DIR)
        self._home_dir = Path(home_dir) if home_dir is not None else util.get_home_dir()
        self._user_config_path = self._home_dir / USER_CONFIG
        self._system_config_path = self._etc_eden_dir / SYSTEM_CONFIG
        self._interpolate_dict = interpolate_dict

        # TODO: We should eventually read the default config_dir path from the config
        # files rather than always using ~/local/.eden
        #
        # We call resolve() to resolve any symlinks in the config directory location.
        # This is particularly important when starting edenfs, since edenfs in some
        # cases will try to access this path as root (e.g., when creating bind mounts).
        # In some cases this path may traverse symlinks that are readable as the
        # original user but not as root: this can happen if the user has a home
        # directory on NFS, which may not be readable as root.
        if config_dir:
            self._config_dir = Path(config_dir)
        elif sys.platform == "win32":
            self._config_dir = self._home_dir / ".eden"
        else:
            self._config_dir = self._home_dir / "local" / ".eden"

        self._config_dir = self._config_dir.resolve(strict=False)

    def __repr__(self) -> str:
        return f"EdenInstance({self._config_dir!r})"

    @property
    def state_dir(self) -> Path:
        return self._config_dir

    @property
    def etc_eden_dir(self) -> Path:
        return self._etc_eden_dir

    @property
    def home_dir(self) -> Path:
        return self._home_dir

    @property
    def user_config_path(self) -> Path:
        return self._user_config_path

    def read_configs(self, paths: List[Path]) -> configutil.EdenConfigParser:
        """
        reads all files specified in paths and parses the configs,
        skips any files which are not found
        """
        parser = configutil.EdenConfigParser(
            interpolation=configinterpolator.EdenConfigInterpolator(
                self._config_variables
            )
        )
        for path in paths:
            try:
                toml_cfg = load_toml_config(path)
            except FileNotFoundError:
                # Ignore missing config files. Eg. user_config_path is optional
                continue
            parser.read_dict(toml_cfg)
        return parser

    def _loadConfig(self) -> configutil.EdenConfigParser:
        """to facilitate templatizing a centrally deployed config, we
        allow a limited set of env vars to be expanded.
        ${HOME} will be replaced by the user's home dir,
        ${USER} will be replaced by the user's login name.
        These are coupled with the equivalent code in
        eden/fs/config/CheckoutConfig.cpp and must be kept in sync.
        """
        return self.read_configs(self.get_rc_files())

    @property
    def _config_variables(self) -> Dict[str, str]:
        if sys.platform == "win32":
            # We don't have user ids on Windows right now.
            # We should update this code if and when we add user id support.
            user_id = 0
            user_name = "USERNAME"
        else:
            user_id = os.getuid()
            user_name = "USER"

        interpolate_dict = self._interpolate_dict
        if interpolate_dict is not None:
            return interpolate_dict
        else:
            return {
                "USER": os.environ.get(user_name, ""),
                "USER_ID": str(user_id),
                "HOME": str(self._home_dir),
            }

    def get_rc_files(self) -> List[Path]:
        result: List[Path] = []
        config_d = self._etc_eden_dir / CONFIG_DOT_D
        try:
            rc_entries = os.listdir(config_d)
        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise
            rc_entries = []

        for name in rc_entries:
            if not name.startswith(".") and name.endswith(".toml"):
                result.append(config_d / name)
        result.sort()
        result.append(self._system_config_path)
        result.append(self._user_config_path)
        return result

    def get_telemetry_logger(self) -> telemetry.TelemetryLogger:
        logger = self._telemetry_logger
        if logger is None:
            logger = self._create_telemetry_logger()
            self._telemetry_logger = logger
        return logger

    def _create_telemetry_logger(self) -> telemetry.TelemetryLogger:
        if "INTEGRATION_TEST" in os.environ:
            return telemetry.NullTelemetryLogger()

        try:
            from eden.fs.cli.facebook import scuba_telemetry  # @manual

            return scuba_telemetry.ScubaTelemetryLogger()
        except (ImportError, NotImplementedError):
            pass

        scribe_cat = self.get_config_value("telemetry.scribe-cat", default="")
        scribe_category = self.get_config_value("telemetry.scribe-category", default="")
        if scribe_cat == "" or scribe_category == "":
            return telemetry.NullTelemetryLogger()
        return telemetry.ExternalTelemetryLogger([scribe_cat, scribe_category])

    def build_sample(
        self, log_type: str, **kwargs: Union[bool, int, str, float, Set[str]]
    ) -> telemetry.TelemetrySample:
        return self.get_telemetry_logger().new_sample(log_type, **kwargs)

    def log_sample(
        self, log_type: str, **kwargs: Union[bool, int, str, float, Set[str]]
    ) -> None:
        self.get_telemetry_logger().log(log_type, **kwargs)

    def get_running_version_parts(self) -> Tuple[str, str]:
        """Get a tuple containing (version, release) of the currently running EdenFS
        daemon.

        The version and release strings will both be the empty string if a development
        build of EdenFS is being used.

        Throws an EdenNotRunningError if EdenFS does not currently appear to be running.
        """
        bi = self.get_server_build_info()
        return (
            bi.get("build_package_version", ""),
            bi.get("build_package_release", ""),
        )

    def get_current_and_running_versions(self) -> Tuple[str, Optional[str]]:
        try:
            running = self.get_running_version()
        except legacy.EdenNotRunningError:
            # return None if EdenFS does not currently appear to be running
            running = None
        return version.get_current_version(), running

    def get_running_version(self) -> str:
        """Get a human-readable string representation of the currently running EdenFS
        version.

        Will return the string "-" if a dev build of EdenFS is being used.

        Throws an EdenNotRunningError if EdenFS does not currently appear to be running.
        """
        return version.format_eden_version(self.get_running_version_parts())

    def get_config_value(self, key: str, default: str) -> str:
        parser = self._loadConfig()
        section, option = key.split(".", 1)
        return parser.get_str(section, option, default=default)

    def get_config_bool(self, key: str, default: bool) -> bool:
        parser = self._loadConfig()
        section, option = key.split(".", 1)
        return parser.get_bool(section, option, default=default)

    def print_full_config(self, out: IO[bytes]) -> None:
        parser = self._loadConfig()
        data: Dict[str, Mapping[str, str]] = {}
        for section in parser.sections():
            data[section] = parser.get_section_str_to_any(section)
        out.write(toml.dumps(data).encode())

    def get_mount_paths(self) -> List[str]:
        """Return the paths of the set mount points stored in config.json"""
        return [str(path) for path in self._get_directory_map().keys()]

    def get_thrift_client(self, timeout: Optional[float] = None) -> "client.EdenClient":
        return client.create_thrift_client(
            eden_dir=str(self._config_dir),
            timeout=timeout,
        )

    def get_thrift_client_legacy(
        self, timeout: Optional[float] = None
    ) -> legacy.EdenClient:
        return legacy.create_thrift_client(
            eden_dir=str(self._config_dir),
            timeout=timeout,
        )

    def get_checkout_info(self, path: Union[Path, str]) -> typing.Mapping[str, str]:
        """
        Given a path to a checkout, return a dictionary containing diagnostic
        information about it.
        """
        path = Path(path).resolve(strict=False)
        client_dir = self._get_client_dir_for_mount_point(path)
        checkout = EdenCheckout(self, path, client_dir)
        return self.get_checkout_info_from_checkout(checkout)

    def get_checkout_info_from_checkout(
        self, checkout: "EdenCheckout"
    ) -> typing.Mapping[str, str]:
        checkout_config = checkout.get_config()

        error = None
        snapshot = None
        try:
            snapshot = checkout.get_snapshot()
        except Exception as ex:
            error = ex

        ret = collections.OrderedDict(
            [
                ("mount", str(checkout.path)),
                ("scm_type", checkout_config.scm_type),
                ("state_dir", str(checkout.state_dir)),
                ("mount_protocol", checkout_config.mount_protocol),
            ]
        )

        if snapshot is not None:
            ret["checked_out_revision"] = snapshot.last_checkout_hash
            ret["working_copy_parent"] = snapshot.working_copy_parent
        if error is not None:
            ret["error"] = str(error)

        return ret

    def get_mounts(self) -> Dict[Path, ListMountInfo]:
        try:
            with self.get_thrift_client_legacy() as client:
                thrift_mounts = client.listMounts()
        except EdenNotRunningError:
            thrift_mounts = []

        config_mounts = self.get_checkouts()
        return self._combine_mount_info(thrift_mounts, config_mounts)

    @classmethod
    def _combine_mount_info(
        cls,
        thrift_mounts: List[ThriftMountInfo],
        config_checkouts: List["EdenCheckout"],
    ) -> Dict[Path, ListMountInfo]:
        mount_points: Dict[Path, ListMountInfo] = {}

        for thrift_mount in thrift_mounts:
            path = Path(os.fsdecode(thrift_mount.mountPoint))
            # Older versions of EdenFS did not report the state field.
            # If it is missing, set it to RUNNING.
            state = (
                thrift_mount.state
                if thrift_mount.state is not None
                else MountState.RUNNING
            )
            data_dir = Path(os.fsdecode(thrift_mount.edenClientPath))

            # this line is for pyre :(
            raw_backing_repo = thrift_mount.backingRepoPath
            backing_repo = (
                Path(os.fsdecode(raw_backing_repo))
                if raw_backing_repo is not None
                else None
            )

            mount_points[path] = ListMountInfo(
                path=path,
                data_dir=data_dir,
                state=state,
                configured=False,
                backing_repo=backing_repo,
            )

        # Add all mount points listed in the config that were not reported
        # in the thrift call.
        for checkout in config_checkouts:
            mount_info = mount_points.get(checkout.path, None)
            if mount_info is not None:
                if mount_info.backing_repo is None:
                    mount_info = mount_info._replace(
                        backing_repo=checkout.get_config().backing_repo
                    )
                mount_points[checkout.path] = mount_info._replace(configured=True)
            else:
                mount_points[checkout.path] = ListMountInfo(
                    path=checkout.path,
                    data_dir=checkout.state_dir,
                    state=None,
                    configured=True,
                    backing_repo=checkout.get_config().backing_repo,
                )

        return mount_points

    def clone(
        self, checkout_config: CheckoutConfig, path: str, snapshot_id: str
    ) -> None:
        if path in self._get_directory_map():
            raise Exception(
                """\
mount path %s is already configured (see `eden list`). \
Do you want to run `eden mount %s` instead?"""
                % (path, path)
            )

        # Create the mount point directory
        self._create_mount_point_dir(path)

        # Create client directory
        clients_dir = self._get_clients_dir()
        clients_dir.mkdir(parents=True, exist_ok=True)
        client_dir = self._create_client_dir_for_path(clients_dir, path)

        # Store snapshot ID
        checkout = EdenCheckout(self, Path(path), Path(client_dir))
        if snapshot_id:
            checkout.save_snapshot(snapshot_id)
        else:
            raise Exception("snapshot id not provided")

        checkout.save_config(checkout_config)

        # Prepare to mount
        mount_info = eden_ttypes.MountArgument(
            mountPoint=os.fsencode(path),
            edenClientPath=os.fsencode(client_dir),
            readOnly=False,
        )
        with self.get_thrift_client_legacy() as client:
            client.mount(mount_info)

        self._post_clone_checkout_setup(checkout, snapshot_id)

        # Add mapping of mount path to client directory in config.json
        self._add_path_to_directory_map(Path(path), os.path.basename(client_dir))

    def _create_mount_point_dir(self, path: str) -> None:
        # Create the directory
        try:
            os.makedirs(path)
        except OSError as e:
            if e.errno != errno.EEXIST:
                raise

            # If the path already exists, make sure it is an empty directory.
            # listdir() will throw its own error if the path is not a directory.
            if len(os.listdir(path)) > 0:
                raise Exception(
                    f'The directory "{path}" already exist on disk. Use `eden rm` if this is an old EdenFS clone to remove it.'
                )

        # On non-Windows platforms, put a README file in the mount point directory.
        # This will be visible to users when the EdenFS checkout is not mounted,
        # and will contain instructions for how to get the checkout re-mounted.
        #
        # On Windows anything we put in this directory will be visible in the checkout
        # itself, so we don't want to put a README file here.
        if sys.platform != "win32":
            self._create_checkout_readme_file(path)

    def _create_checkout_readme_file(self, path: str) -> None:
        help_path = Path(path) / NOT_MOUNTED_README_PATH
        site_readme_path = self._etc_eden_dir / NOT_MOUNTED_SITE_SPECIFIC_README_PATH
        help_contents: Optional[str] = NOT_MOUNTED_DEFAULT_TEXT
        try:
            # Create a symlink to the site-specific readme file.  This helps ensure that
            # users will see up-to-date contents if the site-specific file is updated
            # later.
            with site_readme_path.open("r") as f:
                try:
                    help_path.symlink_to(site_readme_path)
                    help_contents = None
                except OSError as ex:
                    # EPERM can indicate that the underlying filesystem does not support
                    # symlinks.  Read the contents from the site-specific file in this
                    # case.  We will copy them into the file instead of making a
                    # symlink.
                    if ex.errno == errno.EPERM:
                        help_contents = f.read()
                    else:
                        raise
        except OSError as ex:
            if ex.errno == errno.ENOENT:
                # If the site-specific readme file does not exist use default contents
                help_contents = NOT_MOUNTED_DEFAULT_TEXT
            else:
                raise

        if help_contents is not None:
            with help_path.open("w") as f:
                f.write(help_contents)
                if sys.platform != "win32":
                    os.fchmod(f.fileno(), 0o444)

    def _create_client_dir_for_path(self, clients_dir: Path, path: str) -> Path:
        """Tries to create a new subdirectory of clients_dir based on the
        basename of the specified path. Tries appending an increasing sequence
        of integers to the basename if there is a collision until it finds an
        available directory name.
        """
        basename = os.path.basename(path)
        if basename == "":
            raise Exception("Suspicious attempt to clone into: %s" % path)

        i = 0
        while True:
            if i == 0:
                dir_name = basename
            else:
                dir_name = f"{basename}-{i}"

            client_dir = clients_dir / dir_name
            try:
                client_dir.mkdir()
                return client_dir
            except OSError as e:
                if e.errno == errno.EEXIST:
                    # A directory with the specified name already exists: try
                    # again with the next candidate name.
                    i += 1
                    continue
                raise

    def _post_clone_checkout_setup(
        self, checkout: "EdenCheckout", commit_id: str
    ) -> None:
        # First, check to see if the post-clone setup has been run successfully
        # before.
        clone_success_path = checkout.state_dir / CLONE_SUCCEEDED
        is_initial_mount = not clone_success_path.is_file()
        if is_initial_mount and checkout.get_config().scm_type == "hg":
            from . import hg_util

            hg_util.setup_hg_dir(checkout, commit_id)

        clone_success_path.touch()

        if checkout.get_config().scm_type == "hg":
            env = os.environ.copy()
            # These are set by the par machinery and interfere with Mercurial's
            # own dynamic library loading.
            env.pop("DYLD_INSERT_LIBRARIES", None)
            env.pop("DYLD_LIBRARY_PATH", None)

            subprocess.check_call(
                [
                    os.environ.get("EDEN_HG_BINARY", "hg"),
                    "debugedenrunpostupdatehook",
                    "-R",
                    str(checkout.path),
                ],
                env=env,
            )

    def mount(self, path: Union[Path, str], read_only: bool) -> int:
        # Load the config info for this client, to make sure we
        # know about the client.
        path = Path(path).resolve(strict=False)
        client_dir = self._get_client_dir_for_mount_point(path)
        checkout = EdenCheckout(self, path, client_dir)

        # Call checkout.get_config() for the side-effect of it raising an
        # Exception if the config is in an invalid state.
        checkout.get_config()

        # Make sure the mount path exists
        path.mkdir(parents=True, exist_ok=True)

        # Check if it is already mounted.
        try:
            root = path / ".eden" / "root"
            target = os.readlink(root)
            if Path(target) == path:
                print_stderr(
                    f"ERROR: Mount point in use! {path} is already mounted by EdenFS."
                )
                return 1
            else:
                # If we are here, MOUNT/.eden/root is a symlink, but it does not
                # point to MOUNT. This suggests `path` is a subdirectory of an
                # existing mount, though we should never reach this point
                # because _get_client_dir_for_mount_point() above should have
                # already thrown an exception. We return non-zero here just in
                # case.
                print_stderr(
                    f"ERROR: Mount point in use! "
                    f"{path} is already mounted by EdenFS as part of {root}."
                )
                return 1
        except OSError as ex:
            # - ENOENT is expected if the mount is not mounted.
            # - We'll get ENOTCONN if the directory was not properly unmounted from a
            #   previous EdenFS instance.  Remounting over this directory is okay (even
            #   though ideally we would unmount the old stale mount point to clean it
            #   up).
            # - EINVAL can happen if .eden/root isn't a symlink.  This isn't expected
            #   in most circumstances, but it does mean that the directory isn't
            #   currently an EdenFS checkout.
            err = ex.errno
            if err not in (errno.ENOENT, errno.ENOTCONN, errno.EINVAL):
                raise

        # Ask eden to mount the path
        mount_info = eden_ttypes.MountArgument(
            mountPoint=bytes(path), edenClientPath=bytes(client_dir), readOnly=read_only
        )

        try:
            with self.get_thrift_client_legacy() as client:
                client.mount(mount_info)
        except eden_ttypes.EdenError as ex:
            if "already mounted" in str(ex):
                print_stderr(
                    f"ERROR: Mount point in use! {path} is already mounted by EdenFS."
                )
                return 1
            raise

        return 0

    def unmount(self, path: str) -> None:
        """Ask edenfs to unmount the specified checkout."""
        # In some cases edenfs can take a long time unmounting while it waits for
        # inodes to become unreferenced.  Ideally we should have edenfs timeout and
        # forcibly clean up the mount point in this situation.
        #
        # For now at least time out here so the CLI commands do not hang in this
        # case.
        with self.get_thrift_client_legacy(timeout=15) as client:
            client.unmount(os.fsencode(path))

    def get_handle_path(self) -> Optional[Path]:
        handle = shutil.which("handle.exe")
        if handle:
            return Path(handle)
        return None

    def check_handle(self, mount: Path) -> None:
        handle = self.get_handle_path()

        if not handle:
            return

        print(f"Checking handle.exe for processes using '{mount}'...")
        print("Press ctrl+c to skip.")
        try:
            output = subprocess.check_output([handle, "-nobanner", mount])
        except KeyboardInterrupt:
            print("Handle check interrupted.\n")
            print("If you want to find out which process is still using the repo, run:")
            print(f"    handle.exe {mount}\n")
            return
        parsed = [
            line.split() for line in output.decode(errors="ignore").splitlines() if line
        ]
        non_edenfs_process = any(filter(lambda x: x[0].lower() != "edenfs.exe", parsed))

        # When no handle is found in the repo, handle.exe will report `"No
        # matching handles found."`, which will be 4 words.
        if not non_edenfs_process or not parsed or len(parsed[0]) == 4:
            # Nothing other than edenfs.exe is holding handles to files from
            # the repo, we can proceed with the removal
            return

        print(
            "The following processes are still using the repo, please terminate them.\n"
        )

        for executable, _, pid, _, _type, _, path in parsed:
            print(f"{executable}({pid}): {path}")

        print()
        return

    def destroy_mount(
        self, path: Union[Path, str], preserve_mount_point: bool = False
    ) -> None:
        """Delete the specified mount point from the configuration file and remove
        the mount directory, if it exists.

        This should normally be called after unmounting the mount point.
        """
        path = Path(path)
        shutil.rmtree(self._get_client_dir_for_mount_point(path))
        self._remove_path_from_directory_map(path)

    def cleanup_mount(self, path: Path, preserve_mount_point: bool = False) -> None:
        if sys.platform != "win32":
            # Delete the mount point
            # It should normally contain the readme file that we put there, but nothing
            # else.  We only delete these specific files for now rather than using
            # shutil.rmtree() to avoid deleting files we did not create.
            #
            # Previous versions of EdenFS made the mount point directory read-only
            # as part of "eden clone".  Make sure it is writable now so we can clean it up.
            path.chmod(0o755)
            try:
                (path / NOT_MOUNTED_README_PATH).unlink()
            except OSError as ex:
                if ex.errno != errno.ENOENT:
                    raise
            if not preserve_mount_point:
                path.rmdir()
        else:
            # On Windows, the mount point contains ProjectedFS placeholder and
            # files, remove all of them.

            # Embedded Python somehow cannot handle long path on Windows even if
            # it is enabled in the system. Prepending `\\?\` will let Windows
            # API to handle long path.
            windows_prefix = b"\\\\?\\"
            shutil.rmtree(windows_prefix + os.fsencode(path), ignore_errors=True)
            if not path.exists():
                return

            # Somehow, we couldn't remove some of the files, sleep a bit and retry
            time.sleep(0.5)

            errors = []

            def collect_errors(_f, path, ex):
                errors.append((path, ex[1]))

            shutil.rmtree(windows_prefix + os.fsencode(path), onerror=collect_errors)
            if not path.exists():
                return

            print(f"Failed to remove {path}, the following files couldn't be removed:")
            for f in errors:
                print(os.fsdecode(f[0].strip(windows_prefix)))

            print(
                f"""
At this point your EdenFS mount is destroyed, but EdenFS is having
trouble cleaning up leftovers. You will need to manually remove {path}.
"""
            )

            used_by_other = any(
                filter(
                    lambda x: isinstance(x[1], OSError) and x[1].winerror == 32, errors
                )
            )

            if used_by_other:
                if self.get_handle_path():
                    self.check_handle(path)
                else:
                    print(
                        f"""\
    It looks like {path} is still in use by another process. If you need help to
    figure out which process, please try `handle.exe` from sysinternals:

    handle.exe {path}

    """
                    )
                print(
                    f"After terminating the processes, please manually delete {path}."
                )
                print()

            raise errors[0][1]

    def check_health(self, timeout: Optional[float] = None) -> HealthStatus:
        """
        Get the status of the edenfs daemon.

        Returns a HealthStatus object containing health information.
        """
        return util.check_health(
            self.get_thrift_client_legacy, self._config_dir, timeout=timeout
        )

    def check_privhelper_connection(self) -> bool:
        """
        Check if the PrivHelper is accessible.

        Returns True if so, False if not.
        """
        with self.get_thrift_client_legacy() as client:
            return client.checkPrivHelper().connected

    def get_log_path(self) -> Path:
        return self._config_dir / "logs" / "edenfs.log"

    def get_checkout_config_for_path(self, path: str) -> Optional[CheckoutConfig]:
        client_link = os.path.join(path, ".eden", "client")
        try:
            client_dir = os.readlink(client_link)
        except OSError:
            return None

        checkout = EdenCheckout(self, Path(path), Path(client_dir))
        return checkout.get_config()

    def get_checkouts(self) -> List["EdenCheckout"]:
        """Return information about all configured checkouts defined in EdenFS's
        configuration file."""
        dir_map = self._get_directory_map()
        checkouts: List[EdenCheckout] = []
        clients_dir = Path(self._get_clients_dir())
        for mount_path, client_name in dir_map.items():
            checkout_data_dir = clients_dir / client_name
            checkouts.append(EdenCheckout(self, mount_path, checkout_data_dir))

        return checkouts

    def get_hg_repo(self, path: Path) -> util.HgRepo:
        return util.HgRepo(str(path))

    def _directory_map_lock(self) -> BaseFileLock:
        return FileLock(str(self._config_dir / CONFIG_JSON_LOCK))

    def _get_directory_map(self) -> Dict[Path, str]:
        """
        Parse config.json which holds a mapping of mount paths to their
        respective client directory and return contents in a dictionary.
        """
        directory_map = self._config_dir / CONFIG_JSON
        try:
            with directory_map.open() as f:
                data = json.load(f)
        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise
            data = {}
        except json.JSONDecodeError:
            raise Exception(f"invalid JSON data found in {directory_map}")

        if not isinstance(data, dict):
            raise Exception(f"invalid data found in {directory_map}")

        result: Dict[Path, str] = {}
        for k, v in data.items():
            if not isinstance(k, str) or not isinstance(v, str):
                raise Exception(f"invalid data found in {directory_map}")
            result[Path(k)] = v

        return result

    def _add_path_to_directory_map(self, path: Path, dir_name: str) -> None:
        with self._directory_map_lock():
            config_data = self._get_directory_map()
            if path in config_data:
                raise Exception(f"mount path {path} already exists.")
            config_data[path] = dir_name
            self._write_directory_map(config_data)

    def _remove_path_from_directory_map(self, path: Path) -> None:
        with self._directory_map_lock():
            config_data = self._get_directory_map()
            if path in config_data:
                del config_data[path]
                self._write_directory_map(config_data)

    def _write_directory_map(self, config_data: Dict[Path, str]) -> None:
        json_data = {str(path): name for path, name in config_data.items()}
        contents = json.dumps(json_data, indent=2, sort_keys=True) + "\n"
        write_file_atomically(self._config_dir / CONFIG_JSON, contents.encode())

    def _get_client_dir_for_mount_point(self, path: Path) -> Path:
        # The caller is responsible for making sure the path is already
        # a normalized, absolute path.
        assert path.is_absolute()

        config_data = self._get_directory_map()
        if path not in config_data:
            raise Exception(f"could not find mount path {path}")
        return self._get_clients_dir() / config_data[path]

    def _get_clients_dir(self) -> Path:
        return self._config_dir / CLIENTS_DIR

    def get_server_build_info(self) -> Dict[str, str]:
        with self.get_thrift_client_legacy(timeout=3) as client:
            return client.getRegexExportedValues("^build_.*")

    def get_uptime(self) -> datetime.timedelta:
        now = datetime.datetime.now()
        with self.get_thrift_client_legacy(timeout=3) as client:
            since_in_seconds = client.aliveSince()
        since = datetime.datetime.fromtimestamp(since_in_seconds)
        return now - since

    def do_uptime(self, pretty: bool, out: Optional[IO[bytes]] = None) -> None:
        if out is None:
            out = sys.stdout.buffer

        health_info = self.check_health()
        edenfs_pid = health_info.pid
        if edenfs_pid is None:
            running_details = f"{health_info.detail}\n"
            out.write(running_details.encode())
            return

        uptime = self.get_uptime()  # Check if uptime is negative?
        days = uptime.days
        hours, remainder = divmod(uptime.seconds, 3600)
        minutes, seconds = divmod(remainder, 60)

        if pretty:
            if not health_info.is_healthy():
                not_healthy = f"edenfs (pid: {edenfs_pid}) is not healthy\n"
                out.write(not_healthy.encode())

            pretty_uptime = f"edenfs uptime (pid {edenfs_pid}): {datetime.timedelta(days=days, hours=hours, minutes=minutes, seconds=seconds)}\n"
            out.write(pretty_uptime.encode())

        else:
            out.write(b"%dd:%02dh:%02dm:%02ds\n" % (days, hours, minutes, seconds))

    def read_local_config(self) -> configutil.EdenConfigParser:
        return self.read_configs([self.user_config_path])

    def write_local_config(self, config: configutil.EdenConfigParser) -> None:
        """
        Writes TOML config to the local config path.
        NOTE: this method will write an empty file if the config is empty
        """
        write_file_atomically(
            self.user_config_path, toml.dumps(config.to_raw_dict()).encode()
        )


class EdenCheckout:
    """Information about a particular EdenFS checkout."""

    def __init__(self, instance: EdenInstance, path: Path, state_dir: Path) -> None:
        self.instance = instance
        self.path = path
        self.state_dir = state_dir
        self._config: Optional[CheckoutConfig] = None

    def __repr__(self) -> str:
        return f"EdenCheckout({self.instance!r}, {self.path!r}, {self.state_dir!r})"

    def get_relative_path(self, path: Path, already_resolved: bool = False) -> Path:
        """Compute the relative path to a given location inside an EdenFS checkout.

        If the checkout is currently mounted this function is able to correctly resolve
        paths that refer into the checkout via alternative bind mount locations.
        e.g.  if the checkout is located at "/home/user/foo/eden_checkout" but
        "/home/user" is also bind-mounted to "/data/user" this will still be able to
        correctly resolve an input path of "/data/user/foo/eden_checkout/test"
        """
        if not already_resolved:
            path = path.resolve(strict=False)

        # First try using path.relative_to()
        # This should work in the common case
        try:
            return path.relative_to(self.path)
        except ValueError:
            pass

        # path.relative_to() may fail if the checkout is bind-mounted to an alternate
        # location, and the input path points into it using the bind mount location.
        # In this case search upwards from the input path looking for the checkout root.
        try:
            path_stat = path.lstat()
        except OSError as ex:
            raise Exception(
                f"unable to stat {path} to find relative location inside "
                f"checkout {self.path}: {ex}"
            )

        try:
            root_stat = self.path.lstat()
        except OSError as ex:
            raise Exception(f"unable to stat checkout at {self.path}: {ex}")

        if (path_stat.st_dev, path_stat.st_ino) == (root_stat.st_dev, root_stat.st_ino):
            # This is the checkout root
            return Path()

        curdir = path.parent
        path_parts = [path.name]
        while True:
            stat = curdir.lstat()
            if (stat.st_dev, stat.st_ino) == (root_stat.st_dev, root_stat.st_ino):
                path_parts.reverse()
                return Path(*path_parts)

            if curdir.parent == curdir:
                raise Exception(
                    f"unable to determine relative location of {path} "
                    f"inside {self.path}"
                )

            path_parts.append(curdir.name)
            curdir = curdir.parent

    # only for use in unit tests, in production the config should always be read
    # from disk
    def set_config(self, config: CheckoutConfig) -> None:
        self._config = config

    def get_config(self) -> CheckoutConfig:
        config = self._config
        if config is None:
            config = self._read_config()
            self._config = config
        return config

    def save_config(self, checkout_config: CheckoutConfig) -> None:
        # Store information about the mount in the config.toml file.

        # This is a little gross, but only needs to live long enough
        # to swing through migrating folks away from the legacy
        # configuration.

        redirections = {k: str(v) for k, v in checkout_config.redirections.items()}
        config_data = {
            "repository": {
                # TODO: replace is needed to workaround a bug in toml
                "path": str(checkout_config.backing_repo).replace("\\", "/"),
                "type": checkout_config.scm_type,
                "guid": checkout_config.guid,
                "protocol": checkout_config.mount_protocol,
                "case-sensitive": checkout_config.case_sensitive,
                "require-utf8-path": checkout_config.require_utf8_path,
                "enable-tree-overlay": checkout_config.enable_tree_overlay,
                "use-write-back-cache": checkout_config.use_write_back_cache,
            },
            "redirections": redirections,
            "profiles": {
                "active": checkout_config.active_prefetch_profiles,
            },
            "predictive-prefetch": {
                "predictive-prefetch-active": checkout_config.predictive_prefetch_profiles_active,
            },
        }

        if checkout_config.predictive_prefetch_num_dirs:
            config_data["predictive-prefetch"][
                "predictive-prefetch-num-dirs"
            ] = checkout_config.predictive_prefetch_num_dirs

        util.write_file_atomically(
            self._config_path(), toml.dumps(config_data).encode()
        )

        # Update our local config cache
        self._config = checkout_config

    def _config_path(self) -> Path:
        return self.state_dir / MOUNT_CONFIG

    def _read_config(self) -> CheckoutConfig:
        """Returns CheckoutConfig or raises an Exception if the config.toml
        under self.state_dir is not properly formatted or does not exist.
        """
        config_path: Path = self._config_path()
        config = load_toml_config(config_path)
        repo_field = config.get("repository")
        if isinstance(repo_field, dict):
            repository: typing.Mapping[str, str] = repo_field
        else:
            raise Exception(f"{config_path} is missing [repository]")

        def get_field(key: str) -> str:
            value = repository.get(key)
            if isinstance(value, str):
                return value
            raise Exception(f"{config_path} is missing {key} in " "[repository]")

        scm_type = get_field("type")
        if scm_type not in SUPPORTED_REPOS:
            raise Exception(
                f'repository "{config_path}" has unsupported type ' f'"{scm_type}"'
            )

        mount_protocol = repository.get("protocol")
        if not isinstance(mount_protocol, str):
            mount_protocol = "prjfs" if sys.platform == "win32" else "fuse"
        if mount_protocol not in SUPPORTED_MOUNT_PROTOCOLS:
            raise Exception(
                f'repository "{config_path}" has unsupported mount protocol '
                f'"{mount_protocol}"'
            )

        guid = repository.get("guid")
        if not isinstance(guid, str):
            guid = str(uuid.uuid4())

        case_sensitive = repository.get("case-sensitive")
        if not isinstance(case_sensitive, bool):
            # For existing repositories, keep it case sensitive
            case_sensitive = sys.platform != "win32"

        require_utf8_path = repository.get("require-utf8-path")
        if not isinstance(require_utf8_path, bool):
            # Existing repositories may have non-utf8 files, thus allow them.
            require_utf8_path = True

        redirections = {}
        redirections_dict = config.get("redirections")

        if redirections_dict is not None:
            from eden.fs.cli.redirect import RedirectionType  # noqa: F811

            if not isinstance(redirections_dict, dict):
                raise Exception(f"{config_path} has an invalid [redirections] section")
            for key, value in redirections_dict.items():
                if not isinstance(value, str):
                    raise Exception(
                        f"{config_path} has invalid value in "
                        f"[redirections] for {key}: {value} "
                        "(string expected)"
                    )
                try:
                    redirections[key] = RedirectionType.from_arg_str(value)
                except ValueError as exc:
                    raise Exception(
                        f"{config_path} has invalid value in "
                        f"[redirections] for {key}: {value} "
                        f"{str(exc)}"
                    )

        prefetch_profiles = []
        prefetch_profiles_list = config.get("profiles")

        if prefetch_profiles_list is not None:
            prefetch_profiles_list = prefetch_profiles_list.get("active")
            if prefetch_profiles_list is not None:
                if not isinstance(prefetch_profiles_list, list):
                    raise Exception(f"{config_path} has an invalid [profiles] section")
                for profile in prefetch_profiles_list:
                    if not isinstance(profile, str):
                        raise Exception(
                            f"{config_path} has invalid value in "
                            f"[profiles] {profile} (string expected)"
                        )

                    prefetch_profiles.append(profile)

        predictive_prefetch_active = False
        predictive_num_dirs = 0
        predictive_prefetch_profiles_config = config.get("predictive-prefetch")

        if predictive_prefetch_profiles_config is not None:
            predictive_prefetch_active = predictive_prefetch_profiles_config.get(
                "predictive-prefetch-active"
            )
            predictive_num_dirs = predictive_prefetch_profiles_config.get(
                "predictive-prefetch-num-dirs"
            )
            # if predictive-prefetch-num-dirs is not set in config.toml, set
            # predictive_num_dirs to 0 to avoid None != 0 comparisons elsewhere
            if predictive_num_dirs is None:
                predictive_num_dirs = 0

        enable_tree_overlay = repository.get("enable-tree-overlay")
        # Older mount that doesn't have tree overlay setting should remain disabled.
        if not isinstance(enable_tree_overlay, bool):
            enable_tree_overlay = False

        use_write_back_cache = repository.get("use-write-back-cache")
        if not isinstance(use_write_back_cache, bool):
            use_write_back_cache = False

        return CheckoutConfig(
            backing_repo=Path(get_field("path")),
            scm_type=scm_type,
            guid=guid,
            case_sensitive=case_sensitive,
            require_utf8_path=require_utf8_path,
            mount_protocol=mount_protocol,
            redirections=redirections,
            default_revision=(
                repository.get("default-revision") or DEFAULT_REVISION[scm_type]
            ),
            active_prefetch_profiles=prefetch_profiles,
            predictive_prefetch_profiles_active=predictive_prefetch_active,
            predictive_prefetch_num_dirs=predictive_num_dirs,
            enable_tree_overlay=enable_tree_overlay,
            use_write_back_cache=use_write_back_cache,
        )

    def get_snapshot(self) -> SnapshotState:
        """Return the hex version of the parent hash in the SNAPSHOT file."""
        snapshot_path = self.state_dir / SNAPSHOT
        with snapshot_path.open("rb") as f:
            header = f.read(8)
            if header == SNAPSHOT_MAGIC_1:
                decoded_parent = binascii.hexlify(f.read(20)).decode()
                return SnapshotState(
                    working_copy_parent=decoded_parent,
                    last_checkout_hash=decoded_parent,
                )
            elif header == SNAPSHOT_MAGIC_2:
                (bodyLength,) = struct.unpack(">L", f.read(4))
                parent = f.read(bodyLength)
                if len(parent) != bodyLength:
                    raise RuntimeError("SNAPSHOT file too short")
                decoded_parent = parent.decode()
                return SnapshotState(
                    working_copy_parent=decoded_parent,
                    last_checkout_hash=decoded_parent,
                )
            elif header == SNAPSHOT_MAGIC_3:
                (pid,) = struct.unpack(">L", f.read(4))

                (fromLength,) = struct.unpack(">L", f.read(4))
                fromParent = f.read(fromLength)
                if len(fromParent) != fromLength:
                    raise RuntimeError("SNAPSHOT file too short")
                (toLength,) = struct.unpack(">L", f.read(4))
                toParent = f.read(toLength)
                if len(fromParent) != toLength:
                    raise RuntimeError("SNAPSHOT file too short")

                raise InProgressCheckoutError(
                    fromParent.decode(), toParent.decode(), pid
                )
            elif header == SNAPSHOT_MAGIC_4:
                (working_copy_parent_length,) = struct.unpack(">L", f.read(4))
                working_copy_parent = f.read(working_copy_parent_length)
                if len(working_copy_parent) != working_copy_parent_length:
                    raise RuntimeError("SNAPSHOT file too short")
                (checked_out_length,) = struct.unpack(">L", f.read(4))
                checked_out_revision = f.read(checked_out_length)
                if len(checked_out_revision) != checked_out_length:
                    raise RuntimeError("SNAPSHOT file too short")
                return SnapshotState(
                    working_copy_parent=working_copy_parent.decode(),
                    last_checkout_hash=checked_out_revision.decode(),
                )
            else:
                raise RuntimeError("SNAPSHOT file has invalid header")

    def save_snapshot(self, commit_id: str) -> None:
        """Write a new parent commit ID into the SNAPSHOT file."""
        snapshot_path = self.state_dir / SNAPSHOT
        encoded = commit_id.encode()
        write_file_atomically(
            snapshot_path, SNAPSHOT_MAGIC_2 + struct.pack(">L", len(encoded)) + encoded
        )

    def get_backing_repo(self) -> util.HgRepo:
        repo_path = self.get_config().backing_repo
        return self.instance.get_hg_repo(repo_path)

    def activate_profile(
        self,
        profile: str,
        telemetry_sample: telemetry.TelemetrySample,
        force_fetch: bool,
    ) -> int:
        """Add a profile to the config (read the config file and write it back
        with profile added). Returns 0 on sucess and anything else on failure.
        Note this should print information on why it failed if this is not
        returning 0."""

        old_config = self.get_config()
        old_active_profiles = old_config.active_prefetch_profiles
        if profile in old_active_profiles:
            # The profile is already activated so we don't need to update the profile list,
            # but we want to return zero so we continue with the fetch
            if force_fetch:
                return 0
            print(f"Profile {profile} already activated.")
            telemetry_sample.fail(f"Profile {profile} already activated.")
            return 1

        new_active_profiles = old_active_profiles.copy()
        new_active_profiles.append(profile)

        new_config = old_config._replace(
            active_prefetch_profiles=new_active_profiles,
        )

        self.save_config(new_config)
        return 0

    def deactivate_profile(
        self, profile: str, telemetry_sample: telemetry.TelemetrySample
    ) -> int:
        """Remove a profile to the config (read the config file and write it back
        with profile added). Returns 0 on sucess and anything else on failure.
        Note this should print information on why it failed if this is not
        returning 0."""

        old_config = self.get_config()
        old_active_profiles = old_config.active_prefetch_profiles
        if profile not in old_active_profiles:
            print(f"Profile {profile} was not deactivated since it wasn't active.")
            telemetry_sample.fail(
                f"Profile {profile} was not deactivated since it wasn't active."
            )
            return 1

        new_active_profiles = old_active_profiles.copy()
        new_active_profiles.remove(profile)

        new_config = old_config._replace(
            active_prefetch_profiles=new_active_profiles,
        )

        self.save_config(new_config)
        return 0

    def activate_predictive_profile(
        self, num_dirs: int, telemetry_sample: telemetry.TelemetrySample
    ) -> int:
        """Switch on predictive prefetch profiles (read the config file and write it back
        with predictive_prefetch_profiles_active set to True, set or update predictive_prefetch
        _num_dirs if specified). Returns 0 on sucess and anything else on failure.
        Note this should print information on why it failed if this is not
        returning 0."""

        old_config = self.get_config()
        # if predictive prefetch is already activated and num_dirs matches the config, skip activation
        if (
            old_config.predictive_prefetch_profiles_active
            and num_dirs == old_config.predictive_prefetch_num_dirs
        ):
            if num_dirs:
                msg = f"Predictive prefetch profile is already activated with {num_dirs} directories configured."
            else:
                msg = (
                    "Predictive prefetch profile is already activated by default args."
                )
            print(msg)
            telemetry_sample.fail(msg)
            return 1

        new_config = old_config._replace(
            predictive_prefetch_profiles_active=True,
            predictive_prefetch_num_dirs=num_dirs,
        )

        self.save_config(new_config)
        return 0

    def deactivate_predictive_profile(
        self, telemetry_sample: telemetry.TelemetrySample
    ) -> int:
        """Switch off predictive prefetch profiles (read the config file and write it back
        with predictive_profile_profiles_active set to False, unset predictive_prefetch_num_dirs).
        Returns 0 on sucess and anything else on failure. Note this should print information
        on why it failed if this is not returning 0."""

        old_config = self.get_config()
        if not old_config.predictive_prefetch_profiles_active:
            print(
                "Predictive prefetch profile was not deactivated since it wasn't active."
            )
            telemetry_sample.fail(
                "Predictive prefetch profile was not deactivated since it wasn't active."
            )
            return 1

        new_config = old_config._replace(
            predictive_prefetch_profiles_active=False, predictive_prefetch_num_dirs=0
        )

        self.save_config(new_config)
        return 0

    def migrate_mount_protocol(self, new_mount_protocol: str) -> None:
        """
        Migrate this checkout to the new_mount_protocol. This will only take
        effect if EdenFS is restarted. It is recommended to only run this while
        EdenFS is stopped.
        """

        old_config = self.get_config()

        new_config = old_config._replace(mount_protocol=new_mount_protocol)

        self.save_config(new_config)


def detect_nested_checkout(
    path: Union[str, Path],
    instance: EdenInstance,
) -> Tuple[Optional[EdenCheckout], Optional[Path]]:
    """Get a tuple containing (checkout, rel_path) for any checkout that the provided
    path is nested inside of.

    A tuple of (None, None) is returned if the specified path is not nested inside
    any existing checkouts.
    """
    if isinstance(path, str):
        path = Path(path)

    path = path.resolve(strict=False)
    checkout = None
    checkout_state_dir = None

    try:
        # However, we prefer to get the list from the current eden process (if one's running)
        instance.get_running_version()
        checkout_list = instance.get_mounts().items()
    except EdenNotRunningError:  # If EdenFS isn't running, we should fail
        return None, None

    # Checkout list must be sorted so that parent paths are checked first
    rel_path = None
    for checkout_path_str, mount_info in sorted(checkout_list):
        # symlinks could have been added since the mount was added, but
        # we will not worry about this case
        checkout_path = Path(checkout_path_str)
        if path == checkout_path:
            continue
        else:
            try:
                rel_path = path.relative_to(checkout_path)
            except ValueError:
                continue
            checkout_state_dir = mount_info.data_dir
            checkout = EdenCheckout(instance, checkout_path, checkout_state_dir)
            break
    if checkout_state_dir is not None:
        return checkout, rel_path
    else:
        return None, None


def find_eden(
    path: Union[str, Path],
    etc_eden_dir: Optional[str] = None,
    home_dir: Optional[str] = None,
    state_dir: Optional[str] = None,
) -> Tuple[EdenInstance, Optional[EdenCheckout], Optional[Path]]:
    """Look up the EdenInstance and EdenCheckout for a path.

    If the input path points into an EdenFS checkout, this returns a tuple of
    (EdenInstance, EdenCheckout, rel_path), where EdenInstance contains information for
    the edenfs instance serving this checkout, EdenCheckout contains information about
    the checkout, and rel_path contains the relative location of the input path inside
    the checkout.  The checkout does not need to be currently mounted for this to work.

    If the input path does not point inside a known EdenFS checkout, this returns
    (EdenInstance, None, None)
    """
    if isinstance(path, str):
        path = Path(path)

    path = path.resolve(strict=False)

    # First check to see if this looks like a mounted checkout
    eden_state_dir = None
    checkout_root = None
    checkout_state_dir = None
    try:
        if sys.platform != "win32":
            eden_socket_path = os.readlink(path.joinpath(path, ".eden", "socket"))
            eden_state_dir = os.path.dirname(eden_socket_path)

            checkout_root = Path(os.readlink(path.joinpath(".eden", "root")))
            checkout_state_dir = Path(os.readlink(path.joinpath(".eden", "client")))
        else:
            # On Windows, walk the path backwards until both parent and dir
            # point to "C:\"
            curdir = path
            while curdir != curdir.parent:
                try:
                    tomlconfig = toml.load(curdir / ".eden" / "config")
                except FileNotFoundError:
                    curdir = curdir.parent
                    continue

                eden_socket_path = tomlconfig["Config"]["socket"]
                eden_state_dir = os.path.dirname(eden_socket_path)
                checkout_root = Path(tomlconfig["Config"]["root"])
                checkout_state_dir = Path(tomlconfig["Config"]["client"])
                break

    except OSError:
        # We will get an OSError if any of these symlinks do not exist
        # Fall through and we will handle this below.
        pass

    if eden_state_dir is None:
        # Use the state directory argument supplied by the caller.
        # If this is None the EdenInstance constructor will pick the correct location.
        eden_state_dir = state_dir
    elif state_dir is not None:
        # We found a state directory from the checkout and the user also specified an
        # explicit state directory.  Make sure they match.
        _check_same_eden_directory(Path(eden_state_dir), Path(state_dir))

    instance = EdenInstance(
        eden_state_dir, etc_eden_dir=etc_eden_dir, home_dir=home_dir
    )
    checkout: Optional[EdenCheckout] = None
    rel_path: Optional[Path] = None
    if checkout_root is None:
        all_checkouts = instance._get_directory_map()
        for checkout_path_str, checkout_name in all_checkouts.items():
            checkout_path = Path(checkout_path_str)
            try:
                rel_path = path.relative_to(checkout_path)
            except ValueError:
                continue

            checkout_state_dir = instance.state_dir.joinpath(CLIENTS_DIR, checkout_name)
            checkout = EdenCheckout(instance, checkout_path, checkout_state_dir)
            break
        else:
            # This path does not appear to be inside a known checkout
            checkout = None
            rel_path = None
    elif checkout_state_dir is None:
        all_checkouts = instance._get_directory_map()
        checkout_name_value = all_checkouts.get(checkout_root)
        if checkout_name_value is None:
            raise Exception(f"unknown checkout {checkout_root}")
        checkout_state_dir = instance.state_dir.joinpath(
            CLIENTS_DIR, checkout_name_value
        )
        checkout = EdenCheckout(instance, checkout_root, checkout_state_dir)
        rel_path = checkout.get_relative_path(path, already_resolved=True)
    else:
        checkout = EdenCheckout(instance, checkout_root, checkout_state_dir)
        rel_path = checkout.get_relative_path(path, already_resolved=True)

    return (instance, checkout, rel_path)


def eden_instance_from_cmdline(cmdline: List[bytes]) -> EdenInstance:
    try:
        eden_dir_idx = cmdline.index(b"--edenDir") + 1
        eden_dir = Path(cmdline[eden_dir_idx].decode("utf-8"))
    except ValueError:
        eden_dir = None

    try:
        etc_eden_dir_idx = cmdline.index(b"--etcEdenDir") + 1
        etc_eden_dir = Path(cmdline[etc_eden_dir_idx].decode("utf-8"))
    except ValueError:
        etc_eden_dir = None
    try:
        config_path_idx = cmdline.index(b"--configPath") + 1
        config_path = Path(cmdline[config_path_idx].decode("utf-8")).parent
    except ValueError:
        config_path = None

    return EdenInstance(eden_dir, etc_eden_dir, config_path)


def _check_same_eden_directory(found_path: Path, path_arg: Path) -> None:
    s1 = found_path.lstat()
    s2 = path_arg.lstat()
    if (s1.st_dev, s1.st_ino) != (s2.st_dev, s2.st_ino):
        raise Exception(
            f"the specified directory is managed by the edenfs instance at "
            f"{found_path}, which is different from the explicitly requested "
            f"instance at {path_arg}"
        )


def _verify_mount_point(mount_point: str) -> None:
    if os.path.isdir(mount_point):
        return
    parent_dir = os.path.dirname(mount_point)
    if os.path.isdir(parent_dir):
        os.mkdir(mount_point)
    else:
        raise Exception(
            (
                "%s must be a directory in order to mount a client at %s. "
                + "If this is the correct location, run `mkdir -p %s` to create "
                + "the directory."
            )
            % (parent_dir, mount_point, parent_dir)
        )


TomlConfigDict = Mapping[str, Mapping[str, Any]]


def load_toml_config(path: Path) -> TomlConfigDict:
    return typing.cast(TomlConfigDict, toml.load(str(path)))
