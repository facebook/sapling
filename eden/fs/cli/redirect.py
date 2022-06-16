#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import argparse
import enum
import errno
import json
import logging
import os
import shlex
import shutil
import stat
import subprocess
import sys
from pathlib import Path
from typing import Dict, Iterable, Optional, Set

from thrift.Thrift import TApplicationException

from . import cmd_util, mtab, subcmd as subcmd_mod, tabulate
from .buck import is_buckd_running_for_path, stop_buckd_for_path
from .config import CheckoutConfig, EdenCheckout, EdenInstance, load_toml_config
from .subcmd import Subcmd
from .util import mkscratch_bin


redirect_cmd = subcmd_mod.Decorator()

log: logging.Logger = logging.getLogger(__name__)

USER_REDIRECTION_SOURCE = ".eden/client/config.toml:redirections"
REPO_SOURCE = ".eden-redirections"
PLEASE_RESTART = "Please run `eden restart` to pick up the new redirections feature set"
APFS_HELPER = "/usr/local/libexec/eden/eden_apfs_mount_helper"
WINDOWS_SCRATCH_DIR = Path("c:\\open\\scratch")


def have_apfs_helper() -> bool:
    """Determine if the APFS volume helper is installed with appropriate
    permissions such that we can use it to mount things"""
    try:
        st = os.lstat(APFS_HELPER)
        return (st.st_mode & stat.S_ISUID) != 0
    except FileNotFoundError:
        return False


def is_bind_mount(path: Path) -> bool:
    """Detect the most common form of a bind mount in the repo;
    its parent directory will have a different device number than
    the mount point itself.  This won't detect something funky like
    bind mounting part of the repo to a different part."""
    parent = path.parent
    try:
        parent_stat = parent.lstat()
        stat = path.lstat()
        return parent_stat.st_dev != stat.st_dev
    except FileNotFoundError:
        return False


def make_scratch_dir(checkout: EdenCheckout, subdir: Path) -> Path:
    sub = Path("edenfs") / Path("redirections") / subdir

    mkscratch = mkscratch_bin()
    if sys.platform == "win32" and not mkscratch:
        return make_temp_dir(checkout.path, sub)

    return Path(
        subprocess.check_output(
            [
                os.fsdecode(mkscratch),
                "path",
                os.fsdecode(checkout.path),
                "--subdir",
                os.fsdecode(sub),
            ]
        )
        .decode("utf-8")
        .strip()
    )


def make_temp_dir(repo: Path, subdir: Path) -> Path:
    """TODO(zeyi): This is a temporary measurement before we get mkscratch on Windows"""
    escaped = os.fsdecode(repo / subdir).replace("\\", "Z").replace("/", "Z")
    scratch = WINDOWS_SCRATCH_DIR / escaped
    scratch.mkdir(parents=True, exist_ok=True)
    return scratch


class RedirectionState(enum.Enum):
    # Matches the expectations of our configuration as far as we can tell
    MATCHES_CONFIGURATION = "ok"
    # Something mounted that we don't have configuration for
    UNKNOWN_MOUNT = "unknown-mount"
    # We expected it to be mounted, but it isn't
    NOT_MOUNTED = "not-mounted"
    # We expected it to be a symlink, but it is not present
    SYMLINK_MISSING = "symlink-missing"
    # The symlink is present but points to the wrong place
    SYMLINK_INCORRECT = "symlink-incorrect"

    def __str__(self):
        return self.value


class RepoPathDisposition(enum.Enum):
    DOES_NOT_EXIST = 0
    IS_SYMLINK = 1
    IS_BIND_MOUNT = 2
    IS_EMPTY_DIR = 3
    IS_NON_EMPTY_DIR = 4
    IS_FILE = 5

    @classmethod
    def analyze(cls, path: Path) -> "RepoPathDisposition":
        if not path.exists():
            return cls.DOES_NOT_EXIST
        if path.is_symlink():
            return cls.IS_SYMLINK
        if path.is_dir():
            if is_bind_mount(path):
                return cls.IS_BIND_MOUNT
            if is_empty_dir(path):
                return cls.IS_EMPTY_DIR
            return cls.IS_NON_EMPTY_DIR
        return cls.IS_FILE


class RedirectionType(enum.Enum):
    # Linux: a bind mount to a mkscratch generated path
    # macOS: a mounted dmg file in a mkscratch generated path
    # Windows: equivalent to symlink type
    BIND = "bind"
    # A symlink to a mkscratch generated path
    SYMLINK = "symlink"
    UNKNOWN = "unknown"

    def __str__(self):
        return self.value

    @classmethod
    def from_arg_str(cls, arg: str) -> "RedirectionType":
        name_to_value = {"bind": cls.BIND, "symlink": cls.SYMLINK}
        value = name_to_value.get(arg)
        if value:
            return value
        raise ValueError(f"{arg} is not a valid RedirectionType")


def opt_paths_are_equal(a: Optional[Path], b: Optional[Path]) -> bool:
    if a is not None and b is not None:
        return a == b
    if a is None and b is None:
        return True
    # either one or the other is None, but not both, so they are not equal
    return False


class Redirection:
    """Information about an individual redirection"""

    def __init__(
        self,
        repo_path: Path,
        redir_type: RedirectionType,
        target: Optional[Path],
        source: str,
        state: Optional[RedirectionState] = None,
    ) -> None:
        self.repo_path = repo_path
        self.type = redir_type
        self.target = target
        self.source = source
        self.state = state or RedirectionState.MATCHES_CONFIGURATION

    def __eq__(self, b) -> bool:
        return (
            self.repo_path == b.repo_path
            and self.type == b.type
            and opt_paths_are_equal(self.target, b.target)
            and self.source == b.source
            and self.state == b.state
        )

    def as_dict(self, checkout: EdenCheckout) -> Dict[str, str]:
        res = {}
        for name in ["repo_path", "type", "source", "state"]:
            res[name] = str(getattr(self, name))
        res["target"] = str(self.expand_target_abspath(checkout))
        return res

    def expand_target_abspath(self, checkout: EdenCheckout) -> Optional[Path]:
        if self.type == RedirectionType.BIND:
            if have_apfs_helper():
                # Ideally we'd return information about the backing, but
                # it is a bit awkward to determine this in all contexts;
                # prior to creating the volume we don't know anything
                # about where it will reside.
                # After creating it, we could potentially parse the APFS
                # volume information and show something like the backing device.
                # We also have a transitional case where there is a small
                # population of users on disk image mounts; we actually don't
                # have enough knowledge in this code to distinguish between
                # a disk image and an APFS volume (but we can tell whether
                # either of those is mounted elsewhere in this file, provided
                # we have a MountTable to inspect).
                # Given our small user base at the moment, it doesn't seem
                # super critical to have this tool handle all these cases;
                # the same information can be extracted by a human running
                # `mount` and `diskutil list`.
                # So we just return the mount point path when we believe
                # that we can use APFS.
                return checkout.path / self.repo_path
            else:
                return make_scratch_dir(checkout, self.repo_path)
        elif self.type == RedirectionType.SYMLINK:
            return make_scratch_dir(checkout, self.repo_path)
        elif self.type == RedirectionType.UNKNOWN:
            return None
        else:
            raise Exception(f"expand_target_abspath not impl for {self.type}")

    def expand_repo_path(self, checkout: EdenCheckout) -> Path:
        return checkout.path / self.repo_path

    def _dmg_file_name(self, target: Path) -> Path:
        return target / "image.dmg.sparseimage"

    def _bind_mount_darwin(
        self, instance: EdenInstance, checkout_path: Path, target: Path
    ) -> None:
        if have_apfs_helper():
            return self._bind_mount_darwin_apfs(instance, checkout_path, target)
        else:
            return self._bind_mount_darwin_dmg(instance, checkout_path, target)

    def _bind_mount_darwin_apfs(
        self, instance: EdenInstance, checkout_path: Path, target: Path
    ) -> None:
        """Attempt to use an APFS volume for a bind redirection.
        The heavy lifting is part of the APFS_HELPER utility found
        in `eden/scm/exec/eden_apfs_mount_helper/`"""
        mount_path = checkout_path / self.repo_path
        mount_path.mkdir(exist_ok=True, parents=True)
        run_cmd_quietly([APFS_HELPER, "mount", mount_path])

    def _bind_mount_darwin_dmg(
        self, instance: EdenInstance, checkout_path: Path, target: Path
    ) -> None:
        # Since we don't have bind mounts, we set up a disk image file
        # and mount that instead.
        image_file_name = self._dmg_file_name(target)
        total, used, free = shutil.disk_usage(os.fsdecode(target))
        # Specify the size in kb because the disk utilities have weird
        # defaults if the units are unspecified, and `b` doesn't mean
        # bytes!
        total_kb = total / 1024
        mount_path = checkout_path / self.repo_path
        if not image_file_name.exists():
            run_cmd_quietly(
                [
                    "hdiutil",
                    "create",
                    "-size",
                    f"{total_kb}k",
                    "-type",
                    "SPARSE",
                    "-fs",
                    "HFS+",
                    "-volname",
                    f"EdenFS redirection for {mount_path}",
                    image_file_name,
                ]
            )

        run_cmd_quietly(
            [
                "hdiutil",
                "attach",
                image_file_name,
                "-nobrowse",
                "-mountpoint",
                mount_path,
            ]
        )

    def _bind_unmount_darwin(self, checkout: EdenCheckout) -> None:
        mount_path = checkout.path / self.repo_path
        # This will unmount/detach both disk images and apfs volumes
        run_cmd_quietly(["diskutil", "unmount", "force", mount_path])

    def _bind_mount_linux(
        self, instance: EdenInstance, checkout_path: Path, target: Path
    ) -> None:
        abs_mount_path_in_repo = checkout_path / self.repo_path
        with instance.get_thrift_client_legacy() as client:
            if abs_mount_path_in_repo.exists():
                try:
                    # To deal with the case where someone has manually unmounted
                    # a bind mount and left the privhelper confused about the
                    # list of bind mounts, we first speculatively try asking the
                    # eden daemon to unmount it first, ignoring any error that
                    # might raise.
                    client.removeBindMount(
                        os.fsencode(checkout_path), os.fsencode(self.repo_path)
                    )
                except TApplicationException as exc:
                    if exc.type == TApplicationException.UNKNOWN_METHOD:
                        print(PLEASE_RESTART, file=sys.stderr)
                    log.debug("removeBindMount failed; ignoring error", exc_info=True)

            # Ensure that the client directory exists before we try
            # to mount over it
            abs_mount_path_in_repo.mkdir(exist_ok=True, parents=True)
            target.mkdir(exist_ok=True, parents=True)

            try:
                client.addBindMount(
                    os.fsencode(checkout_path),
                    os.fsencode(self.repo_path),
                    os.fsencode(target),
                )
            except TApplicationException as exc:
                if exc.type == TApplicationException.UNKNOWN_METHOD:
                    raise Exception(PLEASE_RESTART)
                raise

    def _bind_unmount_linux(self, checkout: EdenCheckout) -> None:
        with checkout.instance.get_thrift_client_legacy() as client:
            try:
                client.removeBindMount(
                    os.fsencode(checkout.path), os.fsencode(self.repo_path)
                )
            except TApplicationException as exc:
                if exc.type == TApplicationException.UNKNOWN_METHOD:
                    raise Exception(PLEASE_RESTART)
                raise

    def _bind_mount_windows(
        self, instance: EdenInstance, checkout_path: Path, target: Path
    ) -> None:
        self._apply_symlink(checkout_path, target)

    def _bind_unmount_windows(self, checkout: EdenCheckout) -> None:
        repo_path = self.expand_repo_path(checkout)
        repo_path.unlink()

    def _bind_mount(
        self, instance: EdenInstance, checkout_path: Path, target: Path
    ) -> None:
        """Arrange to set up a bind mount"""
        if sys.platform == "darwin":
            return self._bind_mount_darwin(instance, checkout_path, target)

        if "linux" in sys.platform:
            return self._bind_mount_linux(instance, checkout_path, target)

        if sys.platform == "win32":
            return self._bind_mount_windows(instance, checkout_path, target)

        raise Exception(f"don't know how to handle bind mounts on {sys.platform}")

    def _bind_unmount(self, checkout: EdenCheckout) -> None:
        if sys.platform == "darwin":
            return self._bind_unmount_darwin(checkout)

        if "linux" in sys.platform:
            return self._bind_unmount_linux(checkout)

        if sys.platform == "win32":
            return self._bind_unmount_windows(checkout)

        raise Exception(f"don't know how to handle bind mounts on {sys.platform}")

    def remove_existing(
        self, checkout: EdenCheckout, fail_if_bind_mount: bool = False
    ) -> RepoPathDisposition:
        repo_path = self.expand_repo_path(checkout)
        disposition = RepoPathDisposition.analyze(repo_path)
        if disposition == RepoPathDisposition.DOES_NOT_EXIST:
            return disposition

        # If this redirect was setup by buck, we should stop buck
        # prior to unmounting it, as it doesn't currently have a
        # great way to detect that the directories have gone away.
        maybe_buck_project = str(repo_path.parent)
        if is_buckd_running_for_path(maybe_buck_project):
            stop_buckd_for_path(maybe_buck_project)

        if disposition == RepoPathDisposition.IS_SYMLINK:
            repo_path.unlink()
            return RepoPathDisposition.DOES_NOT_EXIST
        if disposition == RepoPathDisposition.IS_BIND_MOUNT:
            if fail_if_bind_mount:
                raise Exception(
                    f"Failed to remove {repo_path} since the bind unmount failed"
                )
            self._bind_unmount(checkout)
            # Now that it is unmounted, re-assess and ideally
            # remove the empty directory that was the mount point
            # To avoid infinite recursion, tell the next call to fail if
            # the disposition is still a bind mount
            return self.remove_existing(checkout, True)
        if disposition == RepoPathDisposition.IS_EMPTY_DIR:
            try:
                repo_path.rmdir()
                return RepoPathDisposition.DOES_NOT_EXIST
            except OSError as err:
                # we won't be able to remove the directory on a read-only file
                # system, but we can still try to mount the redirect over the
                # directory.
                if err.errno == errno.EROFS:
                    return disposition
                else:
                    raise
        return disposition

    def _apply_symlink(self, checkout_path: Path, target: Path) -> None:
        symlink_path = Path(checkout_path / self.repo_path)
        symlink_path.parent.mkdir(exist_ok=True, parents=True)

        if sys.platform != "win32":
            symlink_path.symlink_to(target)
        else:
            # Creating a symlink on Windows is non-atomic, and thus when EdenFS
            # gets the notification about a file being created and then goes on
            # testing what's on disk, it may either find a symlink, or a directory.
            #
            # This is bad for EdenFS for a number of reason. The main one being
            # that EdenFS will attempt to recursively add all the childrens of
            # that directory to the inode hierarchy. If the symlinks points to
            # a very large directory, this can be extremely slow, leading to a
            # very poor user experience.
            #
            # Since these symlinks are created for redirections, we can expect
            # the above to be true.
            #
            # To fix this in a generic way is hard to impossible. One of the
            # approach would be to hack in the PrjfsDispatcherImpl.cpp and
            # sleep a bit when we detect a directory, to make sure that we
            # retest it if this was a symlink. This wouldn't work if the system
            # is overloaded, and it would add a small delay to update/status
            # operation due to these waiting on all pending notifications to be
            # handled.
            #
            # Instead, we chose here to handle it in a local way by forcing the
            # redirection to be created atomically. We first create the symlink
            # in the parent directory of the repository, and then move it
            # inside, which is atomic.
            repo_and_symlink_path = Path(checkout_path.name) / self.repo_path
            temp_symlink_path = checkout_path.parent / Path(
                "Z".join(repo_and_symlink_path.parts)
            )
            # These files should be created by EdenFS only, let's just remove
            # it if it's there.
            temp_symlink_path.unlink(missing_ok=True)
            temp_symlink_path.symlink_to(target)
            os.rename(temp_symlink_path, symlink_path)

    def apply(self, checkout: EdenCheckout) -> None:
        disposition = self.remove_existing(checkout)
        if disposition == RepoPathDisposition.IS_NON_EMPTY_DIR and (
            self.type == RedirectionType.SYMLINK
            or (self.type == RedirectionType.BIND and sys.platform == "win32")
        ):
            # Part of me would like to show this error even if we're going
            # to mount something over the top, but on macOS the act of mounting
            # disk image can leave marker files like `.automounted` in the
            # directory that we mount over, so let's only treat this as a hard
            # error if we want to redirect using a symlink.
            raise Exception(
                f"Cannot redirect {self.repo_path} because it is a "
                "non-empty directory.  Review its contents and remove "
                "it if that is appropriate and then try again."
            )
        if disposition == RepoPathDisposition.IS_FILE:
            raise Exception(f"Cannot redirect {self.repo_path} because it is a file")
        if self.type == RedirectionType.BIND:
            target = self.expand_target_abspath(checkout)
            assert target is not None
            self._bind_mount(checkout.instance, checkout.path, target)
        elif self.type == RedirectionType.SYMLINK:
            target = self.expand_target_abspath(checkout)
            assert target is not None
            self._apply_symlink(checkout.path, target)
        else:
            raise Exception(f"Unsupported redirection type {self.type}")


def load_redirection_profile(path: Path) -> Dict[str, RedirectionType]:
    """Load a redirection profile and return the mapping of path to
    redirection type that it contains.
    """
    config = load_toml_config(path)
    mapping: Dict[str, RedirectionType] = {}
    for k, v in config["redirections"].items():
        mapping[k] = RedirectionType.from_arg_str(v)
    return mapping


def get_configured_redirections(checkout: EdenCheckout) -> Dict[str, Redirection]:
    """Returns the explicitly configured redirection configuration.
    This does not take into account how things are currently mounted;
    use `get_effective_redirections` for that purpose.
    """

    redirs = {}

    config = checkout.get_config()

    # Repo-specified settings have the lowest level of precedence
    repo_redirection_config_file_name = checkout.path / ".eden-redirections"
    if repo_redirection_config_file_name.exists():
        for repo_path, redir_type in load_redirection_profile(
            repo_redirection_config_file_name
        ).items():
            redirs[repo_path] = Redirection(
                Path(repo_path), redir_type, None, REPO_SOURCE
            )

    # User-specific things have the highest precedence
    for repo_path, redir_type in config.redirections.items():
        redirs[repo_path] = Redirection(
            Path(repo_path), redir_type, None, USER_REDIRECTION_SOURCE
        )

    if sys.platform == "win32":
        # Convert path separator to backslash on Windows
        normalized_redirs = {}
        for repo_path, redirection in redirs.items():
            normalized_redirs[repo_path.replace("/", "\\")] = redirection
        return normalized_redirs

    return redirs


def is_strict_subdir(base: bytes, sub_dir: bytes) -> bool:
    return base != sub_dir and sub_dir.startswith(base)


def get_nested_mounts(instance: EdenInstance, checkout_path_bytes: bytes) -> Set[bytes]:
    """
    Find out nested EdenFS mounts in the checkout path
    """
    nested_mounts = set()

    for eden_mount in instance.get_mount_paths():
        eden_mount_path_byte = bytes(Path(eden_mount)) + b"/"
        if is_strict_subdir(checkout_path_bytes, eden_mount_path_byte):
            nested_mounts.add(eden_mount_path_byte)
    return nested_mounts


def in_nested_mount(nested_mounts: Set[bytes], mount_point: bytes) -> bool:
    for nested_mount in nested_mounts:
        if os.path.realpath(nested_mount) == os.path.realpath(
            mount_point
        ) or mount_point.startswith(nested_mount):
            return True
    return False


def get_effective_redirections(
    checkout: EdenCheckout, mount_table: mtab.MountTable, instance: EdenInstance
) -> Dict[str, Redirection]:
    """Computes the complete set of redirections that are currently in effect.
    This is based on the explicitly configured settings but also factors in
    effective configuration by reading the mount table.
    """
    redirs = {}
    checkout_path_bytes = bytes(checkout.path) + b"/"

    nested_mounts = get_nested_mounts(instance, checkout_path_bytes)

    for mount_info in mount_table.read():
        mount_point = mount_info.mount_point
        if not mount_point.startswith(checkout_path_bytes) or in_nested_mount(
            nested_mounts, mount_point
        ):
            continue

        rel_path = os.fsdecode(mount_point[len(checkout_path_bytes) :])

        # The is_bind_mount test may appear to be redundant but it is
        # possible for mounts to layer such that we have:
        #
        # /my/repo    <-- fuse at the top of the vfs
        # /my/repo/buck-out
        # /my/repo    <-- earlier generation fuse at bottom
        #
        # The buck-out bind mount in the middle is visible in the
        # mount table but is not visible via the VFS because there
        # is a different /my/repo mounted over the top.
        #
        # We test whether we can see a mount point at that location
        # before recording it in the effective redirection list so
        # that we don't falsely believe that the bind mount is up.
        if rel_path and is_bind_mount(Path(os.fsdecode(mount_point))):
            redirs[rel_path] = Redirection(
                repo_path=Path(rel_path),
                redir_type=RedirectionType.UNKNOWN,
                target=None,
                source="mount",
                state=RedirectionState.UNKNOWN_MOUNT,
            )

    for rel_path, redir in get_configured_redirections(checkout).items():
        is_in_mount_table = rel_path in redirs
        if is_in_mount_table:
            if redir.type != RedirectionType.BIND:
                redir.state = RedirectionState.UNKNOWN_MOUNT
            # else: we expected them to be in the mount table and they were.
            # we don't know enough to tell whether the mount points where
            # we want it to point, so we just assume that it is in the right
            # state.
        else:
            if redir.type == RedirectionType.BIND and sys.platform != "win32":
                # We expected both of these types to be visible in the
                # mount table, but they were not, so we consider them to
                # be in the NOT_MOUNTED state.
                redir.state = RedirectionState.NOT_MOUNTED
            elif redir.type == RedirectionType.SYMLINK or sys.platform == "win32":
                try:
                    # Resolve to normalize extended-length path on Windows
                    expected_target = redir.expand_target_abspath(checkout)
                    if expected_target:
                        expected_target = expected_target.resolve()
                    symlink_path = os.fsdecode(redir.expand_repo_path(checkout))
                    try:
                        # TODO: replace this with Path.readlink once Python 3.9+
                        target = Path(os.readlink(symlink_path)).resolve()
                    except ValueError as exc:
                        # Windows throws ValueError when the target is not a symlink
                        raise OSError(errno.EINVAL) from exc
                    if target != expected_target:
                        redir.state = RedirectionState.SYMLINK_INCORRECT
                except OSError:
                    # We're considering a variety of errors that might
                    # manifest around trying to read the symlink as meaning
                    # that the symlink is effectively missing, even if it
                    # isn't literally missing.  eg: EPERM means we can't
                    # resolve it, so it is effectively no good.
                    redir.state = RedirectionState.SYMLINK_MISSING
        redirs[rel_path] = redir

    return redirs


def file_size(path: Path) -> int:
    st = path.lstat()
    return st.st_size


def run_cmd_quietly(args, check: bool = True) -> int:
    """Quietly run a command; if successful then its output is entirely suppressed.
    If it fails then raise an exception containing the output/error streams.
    If check=False then print the output and return the exit status"""
    formatted_args = []
    for a in args:
        if isinstance(a, Path):
            # `WindowsPath` is not accepted by subprocess in older version Python
            formatted_args.append(os.fsdecode(a))
        else:
            formatted_args.append(a)

    proc = subprocess.Popen(
        formatted_args, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    stdout, stderr = proc.communicate()
    if proc.returncode != 0:
        cmd = " ".join(shlex.quote(a) for a in formatted_args)
        stdout = stdout.decode("utf-8")
        stderr = stderr.decode("utf-8")
        message = f"{cmd}: Failed with status {proc.returncode}: {stdout} {stderr}"
        if check:
            raise RuntimeError(message)
        print(message, file=sys.stderr)
    return proc.returncode


def apply_redirection_configs_to_checkout_config(
    checkout: EdenCheckout, redirs: Iterable[Redirection]
) -> CheckoutConfig:
    """Translate the redirections into a new CheckoutConfig"""

    config = checkout.get_config()
    redirections = {}
    for r in redirs:
        if r.source != REPO_SOURCE:
            normalized = os.fsdecode(r.repo_path)
            if sys.platform == "win32":
                # TODO: on Windows we replace backslash \ with / since
                # python-toml doesn't escape them correctly when used as key
                normalized = normalized.replace("\\", "/")
            redirections[normalized] = r.type
    return config._replace(
        redirections=redirections,
    )


def is_empty_dir(path: Path) -> bool:
    for ent in path.iterdir():
        if ent not in (".", ".."):
            return False
    return True


def prepare_redirection_list(checkout: EdenCheckout, instance: EdenInstance) -> str:
    mount_table = mtab.new()
    redirs = get_effective_redirections(checkout, mount_table, instance)
    return create_redirection_configs(checkout, redirs.values(), False)


def print_redirection_configs(
    checkout: EdenCheckout, redirs: Iterable[Redirection], use_json: bool
) -> None:
    print(create_redirection_configs(checkout, redirs, use_json))


def create_redirection_configs(
    checkout: EdenCheckout, redirs: Iterable[Redirection], use_json: bool
) -> str:
    redirs = sorted(redirs, key=lambda r: r.repo_path)
    data = [r.as_dict(checkout) for r in redirs]

    if use_json:
        return json.dumps(data)
    else:
        columns = ["repo_path", "type", "target", "source", "state"]
        return tabulate.tabulate(columns, data)


@redirect_cmd("list", "List redirections")
class ListCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--mount", help="The EdenFS mount point path.", default=None
        )
        parser.add_argument(
            "--json",
            help="output in json rather than human readable text",
            action="store_true",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)

        mount_table = mtab.new()
        redirs = get_effective_redirections(checkout, mount_table, instance)
        print_redirection_configs(checkout, redirs.values(), args.json)
        return 0


@redirect_cmd("cleanup-apfs", "Delete stale apfs volumes")
class CleanupApfsCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        if sys.platform != "darwin" or not have_apfs_helper():
            raise Exception(f"Unsupported platform {sys.platform}")

        instance = cmd_util.get_eden_instance(args)
        mounts = instance.get_mounts()

        stdout = subprocess.check_output(
            [
                APFS_HELPER,
                "list-stale-volumes",
            ]
            + [str(path) for path in mounts]
            + ["--json"]
        ).decode("utf-8")
        stale_volumes = json.loads(stdout)

        if not stale_volumes:
            print("No stale volumes detected")
            return 0

        if sys.stdin.isatty():
            volumes_str = "\n  ".join(stale_volumes)
            print(
                f"""\
Warning: this operation will permanently delete the following volumes:
  {volumes_str}
"""
            )
            if not cmd_util.prompt_confirmation("Proceed?"):
                print("Not deleting volumes")
                return 2

        return_code = 0
        for vol in stale_volumes:
            result = subprocess.run(
                [
                    APFS_HELPER,
                    "delete-volume",
                    vol,
                ],
            )
            if result.returncode:
                print(f"Failed to delete volume {vol} due to {result.stderr}")
                return_code = 1
            else:
                print(f"Deleted volume: {vol}")

        return return_code


@redirect_cmd(
    "unmount",
    (
        "Unmount all effective redirection configuration, but preserve "
        "the configuration so that a subsequent fixup will restore it"
    ),
)
class UnmountCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--mount", help="The EdenFS mount point path.", default=None
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)
        mount_table = mtab.new()
        redirs = get_effective_redirections(checkout, mount_table, instance)

        for redir in redirs.values():
            redir.remove_existing(checkout)
            if redir.type == RedirectionType.UNKNOWN:
                continue

        # recompute and display the current state
        redirs = get_effective_redirections(checkout, mount_table, instance)
        ok = True
        for redir in redirs.values():
            if redir.state == RedirectionState.MATCHES_CONFIGURATION:
                ok = False
        return 0 if ok else 1


@redirect_cmd(
    "fixup",
    (
        "Fixup redirection configuration; redirect things that "
        "should be redirected and remove things that should not be redirected"
    ),
)
class FixupCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--mount", help="The EdenFS mount point path.", default=None
        )
        parser.add_argument(
            "--force-remount-bind-mounts",
            help=(
                "Unmount and re-bind mount any bind mount redirections "
                "to ensure that they are pointing to the right place.  "
                "This is not the default behavior in the interest of "
                "preserving kernel caches"
            ),
            action="store_true",
        )
        parser.add_argument(
            "--all-sources",
            help=(
                "By default, paths with source of .eden-redirections will be fixed. "
                "Setting this flag to true will fix paths from all sources."
            ),
            default=True,
            action="store_true",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)
        mount_table = mtab.new()
        redirs = get_effective_redirections(checkout, mount_table, instance)
        for redir in redirs.values():
            if redir.state == RedirectionState.MATCHES_CONFIGURATION and not (
                args.force_remount_bind_mounts and redir.type == RedirectionType.BIND
            ):
                continue

            if redir.source != REPO_SOURCE and not args.all_sources:
                continue

            print(f"Fixing {redir.repo_path}", file=sys.stderr)
            redir.remove_existing(checkout)
            if redir.type == RedirectionType.UNKNOWN:
                continue
            redir.apply(checkout)

        # recompute and display the current state
        redirs = get_effective_redirections(checkout, mount_table, instance)
        ok = True
        for redir in redirs.values():
            if redir.state != RedirectionState.MATCHES_CONFIGURATION:
                ok = False
        return 0 if ok else 1


def resolve_repo_relative_path(checkout_path: Path, repo_rel_path: Path) -> Path:
    """Given a path, verify that it is an appropriate repo-root-relative path
    and return the resolved form of that path.
    The ideal is that they pass in `foo` and we return `foo`, but we also
    allow for the path to be absolute path to `foo`, in which case we resolve
    it and verify that it falls with the repo and then return the relative
    path to `foo`."""

    if repo_rel_path.is_absolute():
        # Well, the original intent was to only interpret paths as relative
        # to the repo root, but it's a bit burdensome to require the caller
        # to correctly relativize for that case, so we'll allow an absolute
        # path to be specified.
        try:
            canonical_repo_path = repo_rel_path.resolve()
            return canonical_repo_path.relative_to(checkout_path)
        except ValueError:
            raise RuntimeError(
                (
                    f"The redirection path `{repo_rel_path}` doesn't resolve "
                    f"to a path inside the repo `{checkout_path}`"
                )
            )

    # Otherwise, the path must be interpreted as being relative to the repo
    # root, so let's resolve that and verify that it lies within the repo
    candidate = (checkout_path / repo_rel_path).resolve()
    try:
        relative = candidate.relative_to(checkout_path)
    except ValueError:
        raise RuntimeError(
            (
                f"The repo-root-relative redirection path `{repo_rel_path}` "
                f"doesn't resolve to a path inside the repo `{checkout_path}`. "
                "Specify either a canonical absolute path to the redirection, or "
                "a canonical (without `..` components) path relative to the "
                f"repository root at `{checkout_path}`."
            )
        )

    # If the resolved and relativized path doesn't match the user-specified
    # path then it means that they either used `..` or a path that resolved
    # through a symlink.  The former is ambiguous, especially because it likely
    # implies that the user is assuming that the path is current working directory
    # relative instead of repo root relative, and the latter is problematic for
    # all of the usual symlink reasons.
    if relative != repo_rel_path:
        raise RuntimeError(
            (
                f"The redirection path `{repo_rel_path}` resolves to `{relative}` "
                "but must be a canonical repo-root-relative path. Specify either a "
                "canonical absolute path to the redirection, or a canonical "
                "(without `..` components) path "
                f"relative to the repository root at `{checkout_path}`."
            )
        )

    return repo_rel_path


@redirect_cmd("add", "Add or change a redirection")
class AddCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--mount", help="The EdenFS mount point path.", default=None
        )
        parser.add_argument(
            "repo_path", help="The path in the repo which should be redirected"
        )
        parser.add_argument(
            "redir_type",
            help="The type of the redirection",
            choices=["bind", "symlink"],
        )
        parser.add_argument(
            "--force-remount-bind-mounts",
            help=(
                "Unmount and re-bind mount any bind mount redirections "
                "to ensure that they are pointing to the right place.  "
                "This is not the default behavior in the interest of "
                "preserving kernel caches"
            ),
            action="store_true",
        )
        parser.add_argument(
            "--strict",
            help=(
                "force bind mount to fail if it would overwrite an "
                "existing directory"
            ),
            action="store_true",
        )

    def _should_return_success_early(
        self,
        redir_type: RedirectionType,
        configured_redirections: Dict[str, Redirection],
        checkout_path: Path,
        repo_path: Path,
    ) -> bool:
        """We should return success early iff:
        1) we're adding a symlink redirection
        2) the symlink already exists
        3) the symlink is already a redirection that's managed by EdenFS
        """
        if redir_type == RedirectionType.SYMLINK:
            # We cannot use resolve_repo_relative_path() because it will essentially
            # attempt to resolve any existing symlinks twice. This causes us to never
            # return the correct path for existing symlinks. Instead, we skip resolving
            # and simply check if the absolute path is relative to the checkout path
            # and if any relative paths are pre-existing configured redirections.
            relative_path = repo_path
            if repo_path.is_absolute():
                try:
                    canonical_repo_path = repo_path
                    relative_path = canonical_repo_path.relative_to(checkout_path)
                except ValueError:
                    raise RuntimeError(
                        (
                            f"The redirection  path `{repo_path}` doesn't resolve "
                            f"to a path inside the repo `{checkout_path}`"
                        )
                    )

            existing_redir = configured_redirections.get(str(relative_path), None)
            if (
                existing_redir
                and existing_redir.type == RedirectionType.SYMLINK
                and existing_redir.repo_path == relative_path
            ):
                return True
        return False

    def run(self, args: argparse.Namespace) -> int:
        redir_type = RedirectionType.from_arg_str(args.redir_type)

        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)

        # Get only the explicitly configured entries for the purposes of the
        # add command, so that we avoid writing out any of the effective list
        # of redirections to the local configuration.  That doesn't matter so
        # much at this stage, but when we add loading in profile(s) later we
        # don't want to scoop those up and write them out to this branch of
        # the configuration.
        redirs = get_configured_redirections(checkout)

        # We are only checking for pre-existing symlinks in this method, so we
        # can use the configured mounts instead of the effective mounts. This is
        # because the symlinks contained in these lists should be the same. I.e.
        # if a symlink is configured, it is also effective.
        if self._should_return_success_early(
            redir_type, redirs, checkout.path, Path(args.repo_path)
        ):
            print("EdenFS managed symlink redirection already exists.")
            return 0

        # We need to query the status of the mounts to catch things like
        # a redirect being configured but unmounted.  This improves the
        # UX in the case where eg: buck is adding a redirect.  Without this
        # we'd hit the skip case below because it is configured, but we wouldn't
        # bring the redirection back online.
        # However, we keep this separate from the `redirs` list below for
        # the reasons stated in the comment above.
        effective_redirs = get_effective_redirections(checkout, mtab.new(), instance)

        try:
            args.repo_path = str(
                resolve_repo_relative_path(checkout.path, Path(args.repo_path))
            )
        except RuntimeError as exc:
            print(str(exc), file=sys.stderr)
            return 1
        redir = Redirection(
            Path(args.repo_path), redir_type, None, USER_REDIRECTION_SOURCE
        )
        existing_redir = effective_redirs.get(args.repo_path, None)
        if (
            existing_redir
            and existing_redir == redir
            and not args.force_remount_bind_mounts
            and existing_redir.state != RedirectionState.NOT_MOUNTED
        ):
            print(
                f"Skipping {redir.repo_path}; it is already configured "
                "(use --force-remount-bind-mounts to force reconfiguring "
                "this redirection)",
                file=sys.stderr,
            )
            return 0

        # We should prevent users from accidentally overwriting existing
        # directories. We only need to check this condition for bind mounts
        # because symlinks should already fail if the target dir exists.
        if redir_type == RedirectionType.BIND and redir.repo_path.is_dir():
            print(
                f"WARNING: {redir.repo_path} already exists.\nMounting over "
                "an existing directory will overwrite its contents.\nYou can "
                "use --strict to prevent overwriting existing directories.\n"
            )
            with instance.get_telemetry_logger().new_sample(
                "redirection_overwrite"
            ) as tel_logger:
                tel_logger.add_string("existing_dir", str(redir.repo_path))
                tel_logger.add_string("checkout", str(checkout.path))
            if args.strict:
                print(
                    f"Not adding redirection {redir.repo_path} because "
                    "the --strict option was used.\nIf you would like "
                    "to add this redirection (not recommended), then "
                    "rerun this command without --strict.",
                    file=sys.stderr,
                )
                return 1

        redir.apply(checkout)

        # We expressly allow replacing an existing configuration in order to
        # support a user with a local ad-hoc override for global- or profile-
        # specified configuration.
        redirs[args.repo_path] = redir
        config = apply_redirection_configs_to_checkout_config(checkout, redirs.values())

        # and persist the configuration so that we can re-apply it in a subsequent
        # call to `edenfsctl redirect fixup`
        checkout.save_config(config)
        return 0


@redirect_cmd("del", "Delete a redirection")
class DelCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--mount", help="The EdenFS mount point path.", default=None
        )
        parser.add_argument(
            "repo_path",
            help="The path in the repo which should no longer be redirected",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)

        redirs = get_configured_redirections(checkout)
        # Note that we're deliberately not using the same validation logic
        # for args.repo_path that we do for the add case for now so that we
        # provide a way to remove bogus redirection paths.  After we've deployed
        # the improved `add` validation for a while, we can use it here also.
        redir = redirs.get(args.repo_path)
        if redir:
            redir.remove_existing(checkout)
            del redirs[args.repo_path]
            config = apply_redirection_configs_to_checkout_config(
                checkout, redirs.values()
            )
            checkout.save_config(config)
            return 0

        redirs = get_effective_redirections(checkout, mtab.new(), instance)
        redir = redirs.get(args.repo_path)
        if redir:
            # This path isn't possible to trigger until we add profiles,
            # but let's be ready for it anyway.
            print(
                f"error: {args.repo_path} is defined by {redir.source} and "
                "cannot be removed using `edenfsctl redirect del {args.repo_path}",
                file=sys.stderr,
            )
            return 1

        print(f"{args.repo_path} is not a known redirection", file=sys.stderr)
        return 1


class RedirectCmd(Subcmd):
    NAME = "redirect"
    HELP = "List and manipulate redirected paths"
    ALIASES = ["redir"]

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        self.add_subcommands(parser, redirect_cmd.commands)

    def run(self, args: argparse.Namespace) -> int:
        # FIXME: I'd rather just show the help here automatically
        print("Specify a subcommand! See `eden redirect --help`", file=sys.stderr)
        return 1
