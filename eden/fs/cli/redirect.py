#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


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

from thrift.python.exceptions import ApplicationError, ApplicationErrorType

from . import configutil, mtab, tabulate
from .config import EdenCheckout, EdenInstance, load_toml_config
from .util import mkscratch_bin


if sys.platform == "win32":
    from .util import remove_unc_prefix


log: logging.Logger = logging.getLogger(__name__)

USER_REDIRECTION_SOURCE = ".eden/client/config.toml:redirections"
REPO_SOURCE = ".eden-redirections"
PLEASE_RESTART = "Please run `eden restart` to pick up the new redirections feature set"
APFS_HELPER = "/usr/local/libexec/eden/eden_apfs_mount_helper"


def have_apfs_helper() -> bool:
    """Determine if the APFS volume helper is installed with appropriate
    permissions such that we can use it to mount things"""
    try:
        st = os.lstat(APFS_HELPER)
        return (st.st_mode & stat.S_ISUID) != 0
    except FileNotFoundError:
        return False


def determine_bind_redirection_type(instance: EdenInstance) -> str:
    """Determine what bind redirection type should be used on macOS.
    There are currently 3 options: "symlink", "apfs" or "dmg". We default
    to the old behavior, "apfs"."""
    config_value = instance.get_config_value(
        "redirections.darwin-redirection-type", "apfs"
    )
    if config_value == "symlink":
        return config_value

    default_type = "apfs" if have_apfs_helper() else "dmg"
    if config_value not in ["symlink", "apfs", "dmg"]:
        print(
            f'darwin redirection type {config_value} must be either "symlink", "apfs" or "dmg". Defaulting to {default_type}.'
        )
        config_value = default_type

    if config_value == "apfs" and not have_apfs_helper():
        print(
            f"cannot use apfs redirections since apfs_helper '{APFS_HELPER}' is not available. Defaulting to {default_type} redirections."
        )
        config_value = default_type

    return config_value


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


def _resolve_symlink_target(symlink_path: Path, *, strict: bool) -> Path:
    target = symlink_path.readlink()
    if not target.is_absolute():
        target = symlink_path.parent / target
    return target.resolve(strict=strict)


def is_valid_symlink(expected_target: Optional[Path], mount_path: Path) -> bool:
    """Detect symlink usage and checks if the symlink is valid
    Symlinks can be used on any system but are the only redirect on Windows"""
    try:
        # Resolve to remove all symlinks in the path
        if expected_target:
            expected_target = expected_target.resolve()
        symlink_path = os.fsdecode(mount_path)
        target = _resolve_symlink_target(Path(symlink_path), strict=False)
        if target != expected_target:
            print(
                f"EXPECTED redirect to resolve to {expected_target}, got {target}",
                file=sys.stderr,
            )
            return False
        return True
    except OSError as exc:
        # We're considering a variety of errors that might
        # manifest around trying to read the symlink as meaning
        # that the symlink is effectively missing, even if it
        # isn't literally missing.  eg: EPERM means we can't
        # resolve it, so it is effectively no good.
        # Throw Exception for logging
        errMsg = f"EXPECTED redirect to resolve to {expected_target}, got {exc}"
        print(
            errMsg,
            file=sys.stderr,
        )
        raise Exception(errMsg)


def is_valid_windows_symlink(expected_target: Optional[Path], mount_path: Path) -> bool:
    """Detect symlink usage and checks if the symlink is valid
    Symlinks can be used on any system but are the only redirect on Windows
    Windows symlinks need some extra handling for path formatting and return values"""
    try:
        # Resolve to normalize extended-length path on Windows and remove all symlinks in the path
        if expected_target:
            expected_target = expected_target.resolve()
        symlink_path = os.fsdecode(mount_path)
        try:
            target = Path(symlink_path).readlink()
            if sys.platform == "win32":
                # readlink returns a UNC path on Windows, so strip the
                # prefix to be able to normalize
                target = remove_unc_prefix(target)
        except ValueError as exc:
            # Windows throws ValueError when the target is not a symlink
            # while NIX throws OSError
            print(
                f"EXPECTED redirect to resolve to {expected_target}, got {exc}",
                file=sys.stderr,
            )
            return False
        if target != expected_target:
            print(
                f"EXPECTED redirect to resolve to {expected_target}, got {target}",
                file=sys.stderr,
            )
            return False
        return True
    except OSError as exc:
        # We're considering a variety of errors that might
        # manifest around trying to read the symlink as meaning
        # that the symlink is effectively missing, even if it
        # isn't literally missing.  eg: EPERM means we can't
        # resolve it, so it is effectively no good.
        # Throw Exception for logging
        errMsg = (f"EXPECTED redirect to resolve to {expected_target}, got {exc}",)
        print(
            errMsg,
            file=sys.stderr,
        )
        raise Exception(errMsg)


def make_scratch_dir(
    checkout: EdenCheckout, subdir: Path, *, no_create: bool = False
) -> Path:
    sub = Path("edenfs") / Path("redirections") / subdir

    mkscratch = mkscratch_bin()
    args = [os.fsdecode(mkscratch)]
    if no_create:
        args.append("--no-create")
    args.extend(
        [
            "path",
            os.fsdecode(checkout.path),
            "--subdir",
            os.fsdecode(sub),
        ]
    )

    return Path(subprocess.check_output(args).decode("utf-8").strip())


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

    # pyre-fixme[3]: Return type must be annotated.
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
        if path.is_symlink():
            return cls.IS_SYMLINK
        if not path.exists():
            return cls.DOES_NOT_EXIST
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

    # pyre-fixme[3]: Return type must be annotated.
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
        # pyre-fixme[4]: Attribute must be annotated.
        self.state = state or RedirectionState.MATCHES_CONFIGURATION

    # pyre-fixme[2]: Parameter must be annotated.
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
        return self._resolve_target_abspath(checkout, no_create=True)

    def ensure_target_abspath(self, checkout: EdenCheckout) -> Optional[Path]:
        return self._resolve_target_abspath(checkout, no_create=False)

    def _resolve_target_abspath(
        self, checkout: EdenCheckout, *, no_create: bool
    ) -> Optional[Path]:
        if self.type == RedirectionType.UNKNOWN:
            return None
        if self.type not in (RedirectionType.BIND, RedirectionType.SYMLINK):
            raise Exception(f"target abspath not impl for {self.type}")
        if self._uses_checkout_path_as_target(checkout):
            return checkout.path / self.repo_path
        return make_scratch_dir(checkout, self.repo_path, no_create=no_create)

    def _uses_checkout_path_as_target(self, checkout: EdenCheckout) -> bool:
        return (
            self.type == RedirectionType.BIND
            and sys.platform == "darwin"
            and not self.expand_repo_path(checkout).is_symlink()
            and determine_bind_redirection_type(checkout.instance) == "apfs"
        )

    def expand_repo_path(self, checkout: EdenCheckout) -> Path:
        return checkout.path / self.repo_path

    def _dmg_file_name(self, target: Path) -> Path:
        return target / "image.dmg.sparseimage"

    def _bind_mount_darwin(
        self, instance: EdenInstance, checkout_path: Path, target: Path
    ) -> None:
        if determine_bind_redirection_type(instance) == "symlink":
            return self._bind_mount_darwin_symlink(instance, checkout_path, target)
        elif determine_bind_redirection_type(instance) == "dmg":
            return self._bind_mount_darwin_dmg(instance, checkout_path, target)
        else:
            return self._bind_mount_darwin_apfs(instance, checkout_path, target)

    def _bind_mount_darwin_symlink(
        self, instance: EdenInstance, checkout_path: Path, target: Path
    ) -> None:
        self._apply_symlink(checkout_path, target)

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
            if not image_file_name.parent.exists():
                image_file_name.parent.mkdir(exist_ok=True, parents=True)
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

        if not mount_path.parent.exists():
            mount_path.parent.mkdir(exist_ok=True, parents=True)
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
        # Only reached for paths that are real mounts: remove_existing unlinks
        # symlink-backed redirections based on disposition analysis first.
        #
        # We use unmount instead of eject here since eject has caused issues
        # by unmounting unrelated apfs volumes in the past. See S325232.
        run_cmd_quietly(["diskutil", "unmount", "force", mount_path])

    def _bind_mount_linux(
        self, instance: EdenInstance, checkout_path: Path, target: Path
    ) -> None:
        abs_mount_path_in_repo = checkout_path / self.repo_path
        with instance.get_thrift_client() as client:
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
                except ApplicationError as exc:
                    if exc.type == ApplicationErrorType.UNKNOWN_METHOD:
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
            except ApplicationError as exc:
                if exc.type == ApplicationErrorType.UNKNOWN_METHOD:
                    raise Exception(PLEASE_RESTART)
                raise

    def _bind_unmount_linux(self, checkout: EdenCheckout) -> None:
        with checkout.instance.get_thrift_client() as client:
            try:
                client.removeBindMount(
                    os.fsencode(checkout.path), os.fsencode(self.repo_path)
                )
            except ApplicationError as exc:
                if exc.type == ApplicationErrorType.UNKNOWN_METHOD:
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
        if disposition == RepoPathDisposition.IS_NON_EMPTY_DIR:
            if self.is_deletable_path(checkout, self.expand_repo_path(checkout)):
                print("Attempting to forcefully remove the directory.")
                if self.forcefully_remove_dir_all(self.expand_repo_path(checkout)):
                    return RepoPathDisposition.DOES_NOT_EXIST
                else:
                    print(
                        f"Failed to delete a non-empty directory (full path `{self.expand_repo_path(checkout)}`)."
                        "\n This happens mostly when some of its files are in use by another process."
                        "\n To detect and kill such processes, follow https://fburl.com/edenfs-redirection-non-empty-directory."
                    )
            else:
                print(
                    f"A non-empty directory (full path `{self.expand_repo_path(checkout)}`) found. Either-"
                    "\n - Try again after reviewing and manually deleting the directory, or "
                    "\n - Run `eden redirect fixup --force` to attempt inline deletion of the directory if none of its files are in use.",
                )
        if disposition == RepoPathDisposition.IS_FILE:
            if self.is_deletable_path(checkout, self.expand_repo_path(checkout)):
                print(
                    "Redirection path found to be a file. Attempting to remove this file."
                )
                if self.remove_file(self.expand_repo_path(checkout)):
                    return RepoPathDisposition.DOES_NOT_EXIST
                else:
                    print(
                        f"Failed to delete the file (full path `{self.expand_repo_path(checkout)}`)."
                        "\n This happens mostly when the file is being used by another process."
                        "\n To detect and kill such processes, follow https://fburl.com/edenfs-redirection-non-empty-directory."
                    )
            else:
                print(
                    f"Redirection path found to be a file (full path `{self.expand_repo_path(checkout)}`). Either-"
                    "\n - Try again after reviewing and manually deleting the file, or "
                    "\n - Run `eden redirect fixup --force` to attempt inline deletion of the file if it is not in use by another process.",
                )
        return disposition

    def forcefully_remove_dir_all(self, path: Path) -> bool:
        try:
            shutil.rmtree(path)
            return True
        except Exception as e:
            print(f"An error occurred while removing directory `{path}`: {e}")
            return False

    def remove_file(self, path: Path) -> bool:
        try:
            os.remove(path)
            return True
        except Exception as e:
            print(f"An error occurred while removing file `{path}`: {e}")
            return False

    def is_deletable_path(self, checkout: EdenCheckout, path: Path) -> bool:
        deletable_paths = checkout.instance.get_config_strs(
            "redirections.redirect-fixup-deletable-paths", default=configutil.Strs()
        )
        for deletable_path in deletable_paths:
            if path.match(deletable_path):
                print(
                    f"\n'{path}' is a path that should only have auto-generated content hence should be safe to delete it to recover your redirections."
                    "\n If this path should not be deleted automatically, please reach out to 'EdenFS Windows Users' (https://fb.workplace.com/groups/edenfswindows) to correct this."
                )
                return True
        return False

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
            target = self.ensure_target_abspath(checkout)
            assert target is not None
            self._bind_mount(checkout.instance, checkout.path, target)
        elif self.type == RedirectionType.SYMLINK:
            target = self.ensure_target_abspath(checkout)
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

    configured_redirections = get_configured_redirections(checkout)
    bind_redirection_uses_symlink = sys.platform == "win32" or (
        sys.platform == "darwin"
        and any(
            redir.type == RedirectionType.BIND
            for redir in configured_redirections.values()
        )
        and instance.get_config_value("redirections.darwin-redirection-type", "apfs")
        == "symlink"
    )

    for rel_path, redir in configured_redirections.items():
        is_in_mount_table = rel_path in redirs
        bind_redirection_is_symlink = (
            sys.platform == "darwin"
            and redir.type == RedirectionType.BIND
            and redir.expand_repo_path(checkout).is_symlink()
        )
        uses_symlink = redir.type == RedirectionType.SYMLINK or (
            redir.type == RedirectionType.BIND
            and (bind_redirection_uses_symlink or bind_redirection_is_symlink)
        )
        if is_in_mount_table:
            # A symlink-backed redirection should never appear in the mount
            # table; if one does, we don't know what is mounted there.
            # Mount-backed binds found in the table are assumed to be mounted
            # correctly.
            if uses_symlink:
                redir.state = RedirectionState.UNKNOWN_MOUNT
        else:
            if redir.type == RedirectionType.BIND and not uses_symlink:
                redir.state = RedirectionState.NOT_MOUNTED
            elif uses_symlink:
                try:
                    # Resolve to verify the target exists and normalize
                    # extended-length paths on Windows.
                    expected_target = redir.expand_target_abspath(checkout)
                    if expected_target:
                        expected_target = expected_target.resolve(strict=True)
                    symlink_path = redir.expand_repo_path(checkout)
                    try:
                        if sys.platform == "win32":
                            target = symlink_path.readlink()
                            target = remove_unc_prefix(target)
                        else:
                            target = _resolve_symlink_target(symlink_path, strict=True)
                    except ValueError as exc:
                        # Windows throws ValueError when the target is not a symlink
                        raise OSError(errno.EINVAL) from exc
                    if target != expected_target:
                        print(
                            f"EXPECTED redirect to resolve to {expected_target}, got {target}",
                            file=sys.stderr,
                        )
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


def check_redirection(redir: Redirection, checkout: EdenCheckout) -> bool:
    """
    Get the redirection for a path.
    """
    mount_path = redir.expand_repo_path(checkout)
    if redir.state == RedirectionState.UNKNOWN_MOUNT:
        # If the redir was in an unknown state before,
        # that means it existed but was not configured.
        # it should have been removed in the remidation step
        # Bind mounts technically aren't used on windows but we're
        # looking for the file not existing here.
        return not is_bind_mount(mount_path)

    uses_symlink = redir.type == RedirectionType.SYMLINK or (
        redir.type == RedirectionType.BIND
        and (
            sys.platform == "win32"
            or (sys.platform == "darwin" and mount_path.is_symlink())
        )
    )
    if uses_symlink:
        expected_target = redir.expand_target_abspath(checkout)
        if sys.platform == "win32":
            return is_valid_windows_symlink(expected_target, mount_path)
        return is_valid_symlink(expected_target, mount_path)
    if redir.type == RedirectionType.BIND:
        return is_bind_mount(mount_path)
    return False


# pyre-fixme[2]: Parameter must be annotated.
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


def is_empty_dir(path: Path) -> bool:
    for ent in path.iterdir():
        if ent not in (".", ".."):
            return False
    return True


def prepare_redirection_list(checkout: EdenCheckout, instance: EdenInstance) -> str:
    mount_table = mtab.new()
    redirs = get_effective_redirections(checkout, mount_table, instance)
    return create_redirection_configs(checkout, redirs.values(), False)


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


def unmount_redirections(instance: EdenInstance, checkout: EdenCheckout) -> int:
    """Unmount all effective redirections for the checkout, preserving the
    configuration so that a subsequent `edenfsctl redirect fixup` restores
    them."""
    mount_table = mtab.new()
    redirs = get_effective_redirections(checkout, mount_table, instance)

    for redir in redirs.values():
        redir.remove_existing(checkout)

    # recompute and verify the current state
    redirs = get_effective_redirections(checkout, mount_table, instance)
    ok = True
    for redir in redirs.values():
        if redir.state == RedirectionState.MATCHES_CONFIGURATION:
            ok = False
    return 0 if ok else 1
