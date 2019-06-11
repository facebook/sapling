#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import binascii
import collections
import configparser
import datetime
import errno
import fcntl
import json
import os
import shutil
import signal
import stat
import subprocess
import tempfile
import time
import types
import typing
from pathlib import Path
from typing import Any, Dict, List, Mapping, Optional, Tuple, Type, Union, cast

import eden.thrift
import facebook.eden.ttypes as eden_ttypes
import toml

from . import configinterpolator, configutil, util
from .util import EdenStartError, HealthStatus, print_stderr, readlink_retry_estale


# Use --etcEdenDir to change the value used for a given invocation
# of the eden cli.
DEFAULT_ETC_EDEN_DIR = "/etc/eden"
# These are INI files that hold config data.
# CONFIG_DOT_D is relative to DEFAULT_ETC_EDEN_DIR, or whatever the
# effective value is for that path
CONFIG_DOT_D = "config.d"
# USER_CONFIG is relative to the HOME dir for the user
USER_CONFIG = ".edenrc"

# These paths are relative to the user's client directory.
CLIENTS_DIR = "clients"
CONFIG_JSON = "config.json"

# These are files in a client directory.
CLONE_SUCCEEDED = "clone-succeeded"
MOUNT_CONFIG = "config.toml"
SNAPSHOT = "SNAPSHOT"
SNAPSHOT_MAGIC = b"eden\x00\x00\x00\x01"

DEFAULT_REVISION = {  # supported repo name -> default bookmark
    "git": "refs/heads/master",
    "hg": ".",
}

SUPPORTED_REPOS = DEFAULT_REVISION.keys()

REPO_FOR_EXTENSION = {".git": "git", ".hg": "hg"}

# Create a readme file with this name in the mount point directory.
# The intention is for this to contain instructions telling users what to do if their
# Eden mount is not currently mounted.
NOT_MOUNTED_README_PATH = "README_EDEN.txt"
# The path under /etc/eden where site-specific contents for the not-mounted README can
# be found.
NOT_MOUNTED_SITE_SPECIFIC_README_PATH = "NOT_MOUNTED_README.txt"
# The default contents for the not-mounted README if a site-specific template
# is not found.
NOT_MOUNTED_DEFAULT_TEXT = """\
This directory is the mount point for a virtual checkout managed by Eden.

If you are seeing this file that means that your repository checkout is not
currently mounted.  This could either be because the edenfs daemon is not
currently running, or it simply does not have this checkout mounted yet.

You can run "eden doctor" to check for problems with Eden and try to have it
automatically remount your checkouts.
"""

assert sorted(REPO_FOR_EXTENSION.values()) == sorted(SUPPORTED_REPOS)


class UsageError(Exception):
    pass


class CheckoutConfig(typing.NamedTuple):
    """Configuration for an Eden checkout. A checkout stores its config in config.toml
    it its state directory (.eden/clients/<checkout_name>/config.toml)

    - backing_repo: The path where the true repo resides on disk.  For mercurial backing
        repositories this does not include the final ".hg" directory component.
    - scm_type: "hg" or "git"
    - bind_mounts: dict where keys are private pathnames under ~/.eden where the
      files are actually stored and values are the relative pathnames in the
      EdenFS mount that maps to them.
    """

    backing_repo: Path
    scm_type: str
    bind_mounts: Dict[str, str]
    default_revision: str


class EdenInstance:
    """This class contains information about a particular edenfs instance.

    It provides APIs for communicating with edenfs over thrift and for examining and
    modifying the list of checkouts managed by this edenfs instance.
    """

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
        elif os.name == "nt":
            self._config_dir = self._home_dir / ".eden"
        else:
            self._config_dir = self._home_dir / "local" / ".eden"

        self._config_dir = self._config_dir.resolve(strict=False)

    def __repr__(self) -> str:
        return f"EdenInstance({self._config_dir!r})"

    @property
    def state_dir(self) -> Path:
        return self._config_dir

    def _loadConfig(self) -> configutil.EdenConfigParser:
        """ to facilitate templatizing a centrally deployed config, we
            allow a limited set of env vars to be expanded.
            ${HOME} will be replaced by the user's home dir,
            ${USER} will be replaced by the user's login name.
            These are coupled with the equivalent code in
            eden/fs/config/CheckoutConfig.cpp and must be kept in sync.
        """
        parser = configutil.EdenConfigParser(
            interpolation=configinterpolator.EdenConfigInterpolator(
                self._config_variables
            )
        )
        for path in self.get_rc_files():
            try:
                toml_cfg = _load_toml_config(path)
            except FileNotFoundError:
                # Ignore missing config files. Eg. user_config_path is optional
                continue
            parser.read_dict(toml_cfg)
        return parser

    @property
    def _config_variables(self) -> Dict[str, str]:
        return (
            self._interpolate_dict
            if self._interpolate_dict is not None
            else {
                "USER": os.environ.get("USER", ""),
                "USER_ID": str(os.getuid()),
                "HOME": str(self._home_dir),
            }
        )

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
            if name.endswith(".toml"):
                result.append(config_d / name)
        result.sort()
        result.append(self._user_config_path)
        return result

    def get_repository_list(
        self, parser: Union[configutil.EdenConfigParser, "ConfigUpdater", None] = None
    ) -> List[str]:
        result = []
        if not parser:
            parser = self._loadConfig()
        for section in parser.sections():
            header = section.split(" ")
            if len(header) == 2 and header[0] == "repository":
                result.append(header[1])
        return sorted(result)

    def get_config_value(self, key: str, default: str) -> str:
        parser = self._loadConfig()
        section, option = key.split(".", 1)
        return parser.get_str(section, option, default=default)

    def should_use_experimental_systemd_mode(self) -> bool:
        # TODO(T33122320): Delete this environment variable when systemd is properly
        # integrated.
        env_var_value = os.getenv("EDEN_EXPERIMENTAL_SYSTEMD")
        if env_var_value == "1":
            return True
        if env_var_value == "0":
            return False

        if self._loadConfig().get_bool(
            "service", "experimental_systemd", default=False
        ):
            return True

        return False

    def get_fallback_systemd_xdg_runtime_dir(self) -> str:
        xdg_runtime_dir = self.get_config_value(
            "service.fallback_systemd_xdg_runtime_dir", default=""
        )
        if xdg_runtime_dir == "":
            user_id = self._config_variables["USER_ID"]
            xdg_runtime_dir = f"/run/user/{user_id}"
        return xdg_runtime_dir

    def print_full_config(self, file: typing.TextIO) -> None:
        parser = self._loadConfig()
        data: Dict[str, Mapping[str, str]] = {}
        for section in parser.sections():
            data[section] = parser.get_section_str_to_any(section)
        toml.dump(data, file)  # pyre-ignore[T39129461]

    def find_config_for_alias(self, alias: str) -> Optional[CheckoutConfig]:
        """Looks through the existing config files and searches for a
        [repository <alias>] section that defines a config:
        - If no such section is found, returns None.
        - If the appropriate section is found, returns a CheckoutConfig if all of
          the fields for the config data are present and well-formed.
        - Otherwise, throws an Exception.
        """
        parser = self._loadConfig()
        repository_header = f"repository {alias}"
        if not parser.has_section(repository_header):
            return None

        bind_mounts_header = f"bindmounts {alias}"
        if parser.has_section(bind_mounts_header):
            # Convert the EdenConfigParser section into a dict so it is JSON
            # serializable for the `eden info` command.
            bind_mounts = dict(parser.get_section_str_to_str(bind_mounts_header))
        else:
            bind_mounts = {}

        scm_type = parser.get_str(repository_header, "type", default="")
        if not scm_type:
            raise Exception(f'repository "{alias}" missing key "type".')
        if scm_type not in SUPPORTED_REPOS:
            raise Exception(f'repository "{alias}" has unsupported type.')

        path = parser.get_str(repository_header, "path", default="")
        if not path:
            raise Exception(f'repository "{alias}" missing key "path".')

        default_revision = (
            parser.get_str(repository_header, "default-revision", default="")
            or parser.get_str("clone", "default-revision", default="")
            or DEFAULT_REVISION[scm_type]
        )

        return CheckoutConfig(
            backing_repo=Path(path),
            scm_type=scm_type,
            bind_mounts=bind_mounts,
            default_revision=default_revision,
        )

    def get_mount_paths(self) -> List[str]:
        """Return the paths of the set mount points stored in config.json"""
        return [str(path) for path in self._get_directory_map().keys()]

    def get_all_client_config_info(self) -> Dict[str, collections.OrderedDict]:
        info = {}
        for path in self.get_mount_paths():
            info[path] = self.get_client_info(path)

        return info

    def get_thrift_client(self) -> eden.thrift.EdenClient:
        return eden.thrift.create_thrift_client(str(self._config_dir))

    def get_client_info(self, path: Union[Path, str]) -> collections.OrderedDict:
        path = Path(path).resolve(strict=False)
        client_dir = self._get_client_dir_for_mount_point(path)
        checkout = EdenCheckout(self, path, client_dir)
        checkout_config = checkout.get_config()
        snapshot = checkout.get_snapshot()

        return collections.OrderedDict(
            [
                ("bind-mounts", checkout_config.bind_mounts),
                ("mount", str(path)),
                ("scm_type", checkout_config.scm_type),
                ("snapshot", snapshot),
                ("client-dir", str(client_dir)),
            ]
        )

    def add_repository(
        self, name: str, repo_type: str, source: str, with_buck: bool = False
    ) -> None:
        # Check if repository already exists
        with ConfigUpdater(self._user_config_path) as config:
            if name in self.get_repository_list(config):
                raise UsageError(
                    """\
repository %s already exists. You will need to edit the ~/.edenrc config file \
by hand to make changes to the repository or remove it."""
                    % name
                )

            # Create a directory for client to store repository metadata
            bind_mounts = {}
            if with_buck:
                bind_mount_name = "buck-out"
                bind_mounts[bind_mount_name] = "buck-out"

            # Add repository to INI file
            config["repository " + name] = {"type": repo_type, "path": source}
            if bind_mounts:
                config["bindmounts " + name] = bind_mounts
            config.save()

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

        # Create bind mounts directories
        bind_mounts_dir = os.path.join(client_dir, "bind-mounts")
        util.mkdir_p(bind_mounts_dir)
        for mount in checkout_config.bind_mounts:
            util.mkdir_p(os.path.join(bind_mounts_dir, mount))

        checkout.save_config(checkout_config)

        # Prepare to mount
        mount_info = eden_ttypes.MountArgument(
            mountPoint=os.fsencode(path), edenClientPath=os.fsencode(client_dir)
        )
        with self.get_thrift_client() as client:
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
                raise OSError(errno.ENOTEMPTY, os.strerror(errno.ENOTEMPTY), path)

        # Populate the directory with a file containing instructions about how to get
        # Eden to remount the checkout.  If Eden is not running or does not have this
        # checkout mounted users will see this file.
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

    def mount(self, path: Union[Path, str]) -> int:
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
            target = readlink_retry_estale(root)
            if Path(target) == path:
                print_stderr(
                    f"ERROR: Mount point in use! {path} is already mounted by Eden."
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
                    f"{path} is already mounted by Eden as part of {root}."
                )
                return 1
        except OSError as ex:
            err = ex.errno
            if err != errno.ENOENT and err != errno.EINVAL:
                raise

        # Ask eden to mount the path
        mount_info = eden_ttypes.MountArgument(
            mountPoint=bytes(path), edenClientPath=bytes(client_dir)
        )
        with self.get_thrift_client() as client:
            client.mount(mount_info)

        return 0

    def unmount(self, path: str) -> None:
        """Ask edenfs to unmount the specified checkout."""
        with self.get_thrift_client() as client:
            # In some cases edenfs can take a long time unmounting while it waits for
            # inodes to become unreferenced.  Ideally we should have edenfs timeout and
            # forcibly clean up the mount point in this situation.
            #
            # For now at least time out here so the CLI commands do not hang in this
            # case.
            client._socket.setTimeout(15000)
            client.unmount(os.fsencode(path))

    def destroy_mount(self, path: Union[Path, str]) -> None:
        """Delete the specified mount point from the configuration file and remove
        the mount directory, if it exists.

        This should normally be called after unmounting the mount point.
        """
        path = Path(path)
        shutil.rmtree(self._get_client_dir_for_mount_point(path))
        self._remove_path_from_directory_map(path)

        # Delete the mount point
        # It should normally contain the readme file that we put there, but nothing
        # else.  We only delete these specific files for now rather than using
        # shutil.rmtree() to avoid deleting files we did not create.
        #
        # Previous versions of Eden made the mount point directory read-only
        # as part of "eden clone".  Make sure it is writable now so we can clean it up.
        path.chmod(0o755)
        try:
            (path / NOT_MOUNTED_README_PATH).unlink()
        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise
        path.rmdir()

    def check_health(self, timeout: Optional[float] = None) -> HealthStatus:
        """
        Get the status of the edenfs daemon.

        Returns a HealthStatus object containing health information.
        """
        return util.check_health(
            lambda: self.get_thrift_client(), self._config_dir, timeout=timeout
        )

    def get_edenfs_start_cmd(
        self,
        daemon_binary: str,
        extra_args: Optional[List[str]] = None,
        takeover: bool = False,
        gdb: bool = False,
        gdb_args: Optional[List[str]] = None,
        strace_file: Optional[str] = None,
        foreground: bool = False,
    ) -> Tuple[List[str], Dict[str, str]]:
        """Get the command and environment to use to start edenfs."""
        if gdb and strace_file is not None:
            raise EdenStartError("cannot run eden under gdb and " "strace together")

        # Compute the command.
        cmd = [
            daemon_binary,
            "--edenfs",
            "--edenDir",
            str(self._config_dir),
            "--etcEdenDir",
            str(self._etc_eden_dir),
            "--configPath",
            str(self._user_config_path),
        ]
        if gdb:
            gdb_args = gdb_args or []
            cmd = ["gdb"] + gdb_args + ["--args"] + cmd
            foreground = True
        if strace_file is not None:
            cmd = ["strace", "-fttT", "-o", strace_file] + cmd
        if extra_args:
            cmd.extend(extra_args)
        if self.should_use_experimental_systemd_mode():
            # TODO(T33122320): Delete this after making 'eden restart' and other
            # callers support systemd mode. (--foreground should never set
            # --experimentalSystemd.)
            cmd.append("--experimentalSystemd")
        if takeover:
            cmd.append("--takeover")
        if foreground:
            cmd.append("--foreground")

        eden_env = self._build_eden_environment()

        # Run edenfs using sudo, unless we already have root privileges,
        # or the edenfs binary is setuid root.
        if os.geteuid() != 0:
            s = os.stat(daemon_binary)
            if not (s.st_uid == 0 and (s.st_mode & stat.S_ISUID)):
                # We need to run edenfs under sudo
                sudo_cmd = ["/usr/bin/sudo"]
                # Add environment variable settings
                # Depending on the sudo configuration, these may not
                # necessarily get passed through automatically even when
                # using "sudo -E".
                for key, value in eden_env.items():
                    sudo_cmd.append("%s=%s" % (key, value))

                cmd = sudo_cmd + cmd

        return cmd, eden_env

    def get_log_path(self) -> Path:
        return self._config_dir / "logs" / "edenfs.log"

    def _build_eden_environment(self) -> Dict[str, str]:
        # Reset $PATH to the following contents, so that everyone has the
        # same consistent settings.
        path_dirs = ["/opt/facebook/hg/bin", "/usr/local/bin", "/bin", "/usr/bin"]

        eden_env = {"PATH": ":".join(path_dirs)}

        # Preserve the following environment settings
        preserve = [
            "USER",
            "LOGNAME",
            "HOME",
            "EMAIL",
            "NAME",
            "ASAN_OPTIONS",
            # When we import data from mercurial, the remotefilelog extension
            # may need to SSH to a remote mercurial server to get the file
            # contents.  Preserve SSH environment variables needed to do this.
            "SSH_AUTH_SOCK",
            "SSH_AGENT_PID",
            "KRB5CCNAME",
        ]

        for name, value in os.environ.items():
            # Preserve any environment variable starting with "TESTPILOT_".
            # TestPilot uses a few environment variables to keep track of
            # processes started during test runs, so it can track down and kill
            # runaway processes that weren't cleaned up by the test itself.
            # We want to make sure this behavior works during the eden
            # integration tests.
            # Similarly, we want to preserve EDENFS_ env vars which are
            # populated by our own test infra to relay paths to important
            # build artifacts in our build tree.
            if name.startswith("TESTPILOT_") or name.startswith("EDENFS_"):
                eden_env[name] = value
            elif name in preserve:
                eden_env[name] = value
            else:
                # Drop any environment variable not matching the above cases
                pass

        return eden_env

    def get_checkout_config_for_path(self, path: str) -> Optional[CheckoutConfig]:
        client_link = os.path.join(path, ".eden", "client")
        try:
            client_dir = readlink_retry_estale(client_link)
        except OSError:
            return None

        checkout = EdenCheckout(self, Path(path), Path(client_dir))
        return checkout.get_config()

    def get_checkouts(self) -> List["EdenCheckout"]:
        """Return information about all configured checkouts defined in Eden's
        configuration file."""
        dir_map = self._get_directory_map()
        checkouts: List[EdenCheckout] = []
        clients_dir = Path(self._get_clients_dir())
        for mount_path, client_name in dir_map.items():
            checkout_data_dir = clients_dir / client_name
            checkouts.append(EdenCheckout(self, mount_path, checkout_data_dir))

        return checkouts

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
        config_data = self._get_directory_map()
        if path in config_data:
            raise Exception("mount path %s already exists." % path)
        config_data[path] = dir_name
        self._write_directory_map(config_data)

    def _remove_path_from_directory_map(self, path: Path) -> None:
        config_data = self._get_directory_map()
        if path in config_data:
            del config_data[path]
            self._write_directory_map(config_data)

    def _write_directory_map(self, config_data: Dict[Path, str]) -> None:
        json_data = {str(path): name for path, name in config_data.items()}
        directory_map = self._config_dir / CONFIG_JSON
        with directory_map.open("w") as f:
            json.dump(json_data, f, indent=2, sort_keys=True)
            f.write("\n")

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
        with self.get_thrift_client() as client:
            try:
                return client.getRegexExportedValues("^build_.*")
            except AttributeError:
                # on macOS currently, we don't know about getRegexExportedValues
                return {}

    def get_uptime(self) -> datetime.timedelta:
        now = datetime.datetime.now()
        with self.get_thrift_client() as client:
            since_in_seconds = client.aliveSince()
        since = datetime.datetime.fromtimestamp(since_in_seconds)
        return now - since


class ConfigUpdater(object):
    """
    A helper class to safely update an eden config file.

    This acquires a lock on the config file, reads it in, and then provide APIs
    to save it back.  This ensures that another process cannot change the file
    in between the time that we read it and when we write it back.

    This also saves the file to a temporary name first, then renames it into
    place, so that the main config file is always in a good state, and never
    has partially written contents.
    """

    def __init__(self, path: Path) -> None:
        self.path = path
        self._lock_path = self.path.with_suffix(".lock")
        self._lock_file: Optional[typing.TextIO] = None
        self.config = configutil.EdenConfigParser()

        # Acquire a lock.
        # This makes sure that another process can't modify the config in the
        # middle of a read-modify-write operation.  (We can't stop a user
        # from manually editing the file while we work, but we can stop
        # other eden CLI processes.)
        self._acquire_lock()
        try:
            toml_cfg = _load_toml_config(self.path)
            self.config.read_dict(toml_cfg)
        except FileNotFoundError:
            pass

    def __enter__(self) -> "ConfigUpdater":
        return self

    def __exit__(
        self,
        exc_type: Optional[Type[BaseException]],
        exc_value: Optional[BaseException],
        exc_traceback: Optional[types.TracebackType],
    ) -> bool:
        self.close()
        return False

    def __del__(self) -> None:
        self.close()

    def sections(self) -> List[str]:
        return self.config.sections()

    def __setitem__(self, key: str, value: Dict[str, Any]) -> None:
        self.config[key] = value

    def _acquire_lock(self) -> None:
        while True:
            self._lock_file = typing.cast(typing.TextIO, open(self._lock_path, "w+"))
            fcntl.flock(self._lock_file.fileno(), fcntl.LOCK_EX)
            # The original creator of the lock file will unlink it when
            # it is finished.  Make sure we grab the lock on the file still on
            # disk, and not an unlinked file.
            st1 = os.fstat(self._lock_file.fileno())
            st2 = os.lstat(self._lock_path)
            if st1.st_dev == st2.st_dev and st1.st_ino == st2.st_ino:
                # We got the real lock
                return

            # We acquired a lock on an old deleted file.
            # Close it, and try to acquire the current lock file again.
            self._lock_file.close()
            self._lock_file = None
            continue

    def _unlock(self) -> None:
        assert self._lock_file is not None
        # Remove the file on disk before we unlock it.
        # This way processes currently waiting in _acquire_lock() that already
        # opened our lock file will see that it isn't the current file on disk
        # once they acquire the lock.
        os.unlink(self._lock_path)
        self._lock_file.close()
        self._lock_file = None

    def close(self) -> None:
        if self._lock_file is not None:
            self._unlock()

    def save(self) -> None:
        if self._lock_file is None:
            raise Exception("Cannot save the config without holding the lock")

        try:
            st = os.stat(self.path)
            perms = st.st_mode & 0o777
        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise
            perms = 0o644

        # Write the contents to a temporary file first, then atomically rename
        # it to the desired destination.  This makes sure the .edenrc file
        # always has valid contents at all points in time.
        prefix = USER_CONFIG + ".tmp."
        dirname = self.path.parent
        tmpf = tempfile.NamedTemporaryFile(
            "w", dir=str(dirname), prefix=prefix, delete=False
        )
        try:
            toml_config = self.config.to_raw_dict()
            toml_data = toml.dumps(typing.cast(Mapping[str, Any], toml_config))
            tmpf.write(toml_data)
            tmpf.close()
            os.chmod(tmpf.name, perms)
            os.rename(tmpf.name, self.path)
        except BaseException:
            # Remove temporary file on error
            try:
                os.unlink(tmpf.name)
            except Exception:
                pass
            raise


class EdenCheckout:
    """Information about a particular Eden checkout."""

    def __init__(self, instance: EdenInstance, path: Path, state_dir: Path) -> None:
        self.instance = instance
        self.path = path
        self.state_dir = state_dir
        self._config: Optional[CheckoutConfig] = None

    def __repr__(self) -> str:
        return f"EdenCheckout({self.instance!r}, {self.path!r}, {self.state_dir!r})"

    def get_relative_path(self, path: Path, already_resolved: bool = False) -> Path:
        """Compute the relative path to a given location inside an eden checkout.

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

        # pyre-fixme[22]: The cast is redundant.
        curdir = cast(Path, path.parent)
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
            # pyre-fixme[22]: The cast is redundant.
            curdir = typing.cast(Path, curdir.parent)

    def get_config(self) -> CheckoutConfig:
        if self._config is None:
            self._config = self._read_config()
        return self._config

    def save_config(self, checkout_config: CheckoutConfig) -> None:
        # Store information about the mount in the config.toml file.
        config_data = {
            "repository": {
                "path": str(checkout_config.backing_repo),
                "type": checkout_config.scm_type,
            },
            "bind-mounts": checkout_config.bind_mounts,
        }
        with self._config_path().open("w") as f:
            # pyre-fixme[6]: Expected `_Writable` for 2nd param but got `IO[]`.
            toml.dump(config_data, f)

        # Update our local config cache
        self._config = checkout_config

    def _config_path(self) -> Path:
        return self.state_dir / MOUNT_CONFIG

    def _read_config(self) -> CheckoutConfig:
        """Returns CheckoutConfig or raises an Exception if the config.toml
        under self.state_dir is not properly formatted or does not exist.
        """
        config_path = self._config_path()
        config = _load_toml_config(config_path)
        repo_field = config.get("repository")
        if isinstance(repo_field, dict):
            repository = repo_field
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

        bind_mounts = {}
        bind_mounts_dict = config.get("bind-mounts")
        if bind_mounts_dict is not None:
            if not isinstance(bind_mounts_dict, dict):
                raise Exception(
                    f"{config_path} has an invalid " "[bind-mounts] section"
                )
            for key, value in bind_mounts_dict.items():
                if not isinstance(value, str):
                    raise Exception(
                        f"{config_path} has invalid value in "
                        f"[bind-mounts] for {key}: {value} "
                        "(string expected)"
                    )
                bind_mounts[key] = value

        return CheckoutConfig(
            backing_repo=Path(get_field("path")),
            scm_type=scm_type,
            bind_mounts=bind_mounts,
            default_revision=(
                repository.get("default-revision") or DEFAULT_REVISION[scm_type]
            ),
        )

    def get_snapshot(self) -> str:
        """Return the hex version of the parent hash in the SNAPSHOT file."""
        snapshot_path = self.state_dir / SNAPSHOT
        with snapshot_path.open("rb") as f:
            assert f.read(8) == SNAPSHOT_MAGIC
            return binascii.hexlify(f.read(20)).decode("utf-8")

    def save_snapshot(self, commid_id: str) -> None:
        """Write a new parent commit ID into the SNAPSOHT file."""
        snapshot_path = self.state_dir / SNAPSHOT
        assert len(commid_id) == 40
        commit_bin = binascii.unhexlify(commid_id)
        # TODO: It would be nicer to write this out atomically using a temporary file
        # followed by a rename.
        snapshot_path.write_bytes(SNAPSHOT_MAGIC + commit_bin)


def find_eden(
    path: Union[str, Path],
    etc_eden_dir: Optional[str] = None,
    home_dir: Optional[str] = None,
    state_dir: Optional[str] = None,
) -> Tuple[EdenInstance, Optional[EdenCheckout], Optional[Path]]:
    """Look up the EdenInstance and EdenCheckout for a path.

    If the input path points into an Eden checkout, this returns a tuple of
    (EdenInstance, EdenCheckout, rel_path), where EdenInstance contains information for
    the edenfs instance serving this checkout, EdenCheckout contains information about
    the checkout, and rel_path contains the relative location of the input path inside
    the checkout.  The checkout does not need to be currently mounted for this to work.

    If the input path does not point inside a known Eden checkout, this returns
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
        eden_socket_path = readlink_retry_estale(path.joinpath(path, ".eden", "socket"))
        eden_state_dir = os.path.dirname(eden_socket_path)

        checkout_root = Path(readlink_retry_estale(path.joinpath(".eden", "root")))
        checkout_state_dir = Path(
            readlink_retry_estale(path.joinpath(".eden", "client"))
        )
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


_TomlConfigDict = Mapping[str, Mapping[str, Any]]


def _load_toml_config(path: Path) -> _TomlConfigDict:
    return typing.cast(_TomlConfigDict, toml.load(str(path)))
