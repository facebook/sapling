#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import enum
import json
import logging
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Dict, Iterable, Optional

from thrift.Thrift import TApplicationException

from . import cmd_util, mtab, subcmd as subcmd_mod, tabulate
from .config import CheckoutConfig, EdenCheckout, EdenInstance, load_toml_config
from .subcmd import Subcmd
from .util import mkdir_p, mkscratch_bin


redirect_cmd = subcmd_mod.Decorator()

log = logging.getLogger(__name__)

LEGACY_BIND_MOUNTS_SOURCE = ".eden/client/config.toml:bind-mounts"
USER_REDIRECTION_SOURCE = ".eden/client/config.toml:redirections"
REPO_SOURCE = ".eden-redirections"
PLEASE_RESTART = "Please run `eden restart` to pick up the new redirections feature set"


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


def make_scratch_dir(checkout: EdenCheckout, subdir: str) -> Path:
    sub = os.path.join("edenfs", "redirections", subdir)
    return Path(
        subprocess.check_output(
            [mkscratch_bin(), "path", checkout.path, "--subdir", sub]
        )
        .decode("utf-8")
        .strip()
    )


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
    # A bind mount specified by the legacy configuration
    LEGACY = "legacy"
    # Linux: a bind mount to a mkscratch generated path
    # macOS: a mounted dmg file in a mkscratch generated path
    BIND = "bind"
    # A symlink to a mkscratch generated path
    SYMLINK = "symlink"
    UNKNOWN = "unknown"

    def __str__(self):
        return self.value

    @classmethod
    def from_arg_str(cls, arg: str) -> "RedirectionType":
        name_to_value = {"legacy": cls.LEGACY, "bind": cls.BIND, "symlink": cls.SYMLINK}
        value = name_to_value.get(arg)
        if value:
            return value
        raise ValueError(f"{arg} is not a valid RedirectionType")


class Redirection:
    """Information about an individual redirection"""

    def __init__(
        self,
        repo_path: Path,
        redir_type: RedirectionType,
        target: Optional[Path],
        source: str,
        state: Optional[RedirectionState] = None,
    ):
        self.repo_path = repo_path
        self.type = redir_type
        self.target = target
        self.source = source
        self.state = state or RedirectionState.MATCHES_CONFIGURATION

    def as_dict(self, checkout: EdenCheckout) -> Dict[str, str]:
        res = {}
        for name in ["repo_path", "type", "source", "state"]:
            res[name] = str(getattr(self, name))
        res["target"] = str(self.expand_target_abspath(checkout))
        return res

    def expand_target_abspath(self, checkout: EdenCheckout) -> Optional[Path]:
        if self.type == RedirectionType.LEGACY:
            assert self.target is not None
            return checkout.state_dir / "bind-mounts" / self.target
        elif self.type in (RedirectionType.SYMLINK, RedirectionType.BIND):
            return make_scratch_dir(checkout, str(self.repo_path))
        elif self.type == RedirectionType.UNKNOWN:
            return None
        else:
            raise Exception(f"expand_target_abspath not impl for {self.type}")

    def expand_repo_path(self, checkout: EdenCheckout) -> Path:
        return checkout.path / self.repo_path

    def _dmg_file_name(self, target: bytes) -> Path:
        return Path(os.fsdecode(target)) / "image.dmg.sparseimage"

    def _bind_mount_darwin(
        self, instance: EdenInstance, checkout_path: bytes, target: bytes
    ):
        if self.type == RedirectionType.LEGACY:
            raise Exception("legacy redirections are not supported on macOS")

        # Since we don't have bind mounts, we set up a disk image file
        # and mount that instead.
        image_file_name = self._dmg_file_name(target)
        total, used, free = shutil.disk_usage(os.fsdecode(target))
        # Specify the size in kb because the disk utilities have weird
        # defaults if the units are unspecified, and `b` doesn't mean
        # bytes!
        total_kb = total / 1024
        mount_path = Path(os.fsdecode(checkout_path)) / self.repo_path
        if not image_file_name.exists():
            subprocess.check_call(
                [
                    "hdiutil",
                    "create",
                    "-size",
                    f"{total_kb}k",
                    "-type",
                    "SPARSE",
                    "-fs",
                    "APFS",
                    "-volname",
                    f"Eden redirection for {mount_path}",
                    str(image_file_name),
                ]
            )

        subprocess.check_call(
            [
                "hdiutil",
                "attach",
                str(image_file_name),
                "-nobrowse",
                "-mountpoint",
                str(mount_path),
            ]
        )

    def _bind_unmount_darwin(self, checkout: EdenCheckout):
        mount_path = checkout.path / self.repo_path
        subprocess.check_call(["hdiutil", "unmount", str(mount_path)])

    def _bind_mount_linux(
        self, instance: EdenInstance, checkout_path: bytes, target: bytes
    ):
        abs_mount_path_in_repo = os.path.join(
            checkout_path, os.fsencode(self.repo_path)
        )
        with instance.get_thrift_client() as client:
            if os.path.exists(abs_mount_path_in_repo):
                try:
                    # To deal with the case where someone has manually unmounted
                    # a bind mount and left the privhelper confused about the
                    # list of bind mounts, we first speculatively try asking the
                    # eden daemon to unmount it first, ignoring any error that
                    # might raise.
                    client.removeBindMount(checkout_path, os.fsencode(self.repo_path))
                except TApplicationException as exc:
                    if exc.type == TApplicationException.UNKNOWN_METHOD:
                        print(PLEASE_RESTART, file=sys.stderr)
                    log.debug("removeBindMount failed; ignoring error", exc_info=True)

            # Ensure that the client directory exists before we try
            # to mount over it
            os.makedirs(abs_mount_path_in_repo, exist_ok=True)
            os.makedirs(target, exist_ok=True)

            try:
                client.addBindMount(checkout_path, os.fsencode(self.repo_path), target)
            except TApplicationException as exc:
                if exc.type == TApplicationException.UNKNOWN_METHOD:
                    raise Exception(PLEASE_RESTART)
                raise

    def _bind_unmount_linux(self, checkout: EdenCheckout):
        with checkout.instance.get_thrift_client() as client:
            try:
                client.removeBindMount(
                    bytes(checkout.path), os.fsencode(self.repo_path)
                )
            except TApplicationException as exc:
                if exc.type == TApplicationException.UNKNOWN_METHOD:
                    raise Exception(PLEASE_RESTART)
                raise

    def _bind_mount(self, instance: EdenInstance, checkout_path: bytes, target: bytes):
        """Arrange to set up a bind mount"""
        if sys.platform == "darwin":
            return self._bind_mount_darwin(instance, checkout_path, target)

        if "linux" in sys.platform:
            return self._bind_mount_linux(instance, checkout_path, target)

        raise Exception(f"don't know how to handle bind mounts on {sys.platform}")

    def _bind_unmount(self, checkout: EdenCheckout):
        if sys.platform == "darwin":
            return self._bind_unmount_darwin(checkout)

        if "linux" in sys.platform:
            return self._bind_unmount_linux(checkout)

        raise Exception(f"don't know how to handle bind mounts on {sys.platform}")

    def remove_existing(self, checkout: EdenCheckout) -> RepoPathDisposition:
        repo_path = self.expand_repo_path(checkout)
        disposition = RepoPathDisposition.analyze(repo_path)
        if disposition == RepoPathDisposition.DOES_NOT_EXIST:
            return disposition
        if disposition == RepoPathDisposition.IS_SYMLINK:
            repo_path.unlink()
            return RepoPathDisposition.DOES_NOT_EXIST
        if disposition == RepoPathDisposition.IS_BIND_MOUNT:
            self._bind_unmount(checkout)
            # Now that it is unmounted, re-assess and ideally
            # remove the empty directory that was the mount point
            return self.remove_existing(checkout)
        if disposition == RepoPathDisposition.IS_EMPTY_DIR:
            repo_path.rmdir()
            return RepoPathDisposition.DOES_NOT_EXIST
        return disposition

    def apply(self, checkout: EdenCheckout):
        disposition = self.remove_existing(checkout)
        if (
            disposition == RepoPathDisposition.IS_NON_EMPTY_DIR
            and self.type == RedirectionType.SYMLINK
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
        if self.type in (RedirectionType.LEGACY, RedirectionType.BIND):
            target = self.expand_target_abspath(checkout)
            assert target is not None
            self._bind_mount(checkout.instance, bytes(checkout.path), bytes(target))
        elif self.type == RedirectionType.SYMLINK:
            symlink_path = Path(os.path.join(checkout.path, self.repo_path))
            mkdir_p(os.path.dirname(symlink_path))
            target = self.expand_target_abspath(checkout)
            assert target is not None
            os.symlink(target, symlink_path)
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

    # Legacy configuration has lowest precedence
    for store_path, repo_path in config.bind_mounts.items():
        redirs[repo_path] = Redirection(
            Path(repo_path),
            RedirectionType.LEGACY,
            Path(store_path),
            LEGACY_BIND_MOUNTS_SOURCE,
        )

    # Repo-specified settings have next level of precedence
    repo_redirection_config_file_name = checkout.path.joinpath(".eden-redirections")
    if os.path.exists(repo_redirection_config_file_name):
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

    return redirs


def get_effective_redirections(
    checkout: EdenCheckout, mount_table: mtab.MountTable
) -> Dict[str, Redirection]:
    """Computes the complete set of redirections that are currently in effect.
    This is based on the explicitly configured settings but also factors in
    effective configuration by reading the mount table.
    """
    redirs = {}
    checkout_path_bytes = bytes(checkout.path) + b"/"
    for mount_info in mount_table.read():
        mount_point = mount_info.mount_point
        if mount_point.startswith(checkout_path_bytes):
            rel_path = os.fsdecode(mount_point[len(checkout_path_bytes) :])
            if rel_path:
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
            if redir.type not in (RedirectionType.LEGACY, RedirectionType.BIND):
                redir.state = RedirectionState.UNKNOWN_MOUNT
            # else: we expected them to be in the mount table and they were.
            # we don't know enough to tell whether the mount points where
            # we want it to point, so we just assume that it is in the right
            # state.
        else:
            if redir.type in (RedirectionType.LEGACY, RedirectionType.BIND):
                # We expected both of these types to be visible in the
                # mount table, but they were not, so we consider them to
                # be in the NOT_MOUNTED state.
                redir.state = RedirectionState.NOT_MOUNTED
            elif redir.type == RedirectionType.SYMLINK:
                try:
                    expected_target = redir.expand_target_abspath(checkout)
                    target = Path(os.readlink(redir.expand_repo_path(checkout)))
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


def apply_redirection_configs_to_checkout_config(
    checkout: EdenCheckout, redirs: Iterable[Redirection]
) -> CheckoutConfig:
    """ Translate the redirections into a new CheckoutConfig """

    config = checkout.get_config()
    bind_mounts = {}
    redirections = {}
    for r in redirs:
        if r.type == RedirectionType.LEGACY:
            bind_mounts[str(r.target)] = str(r.repo_path)
        elif r.source != REPO_SOURCE:
            redirections[str(r.repo_path)] = r.type
    return CheckoutConfig(
        backing_repo=config.backing_repo,
        scm_type=config.scm_type,
        default_revision=config.default_revision,
        bind_mounts=bind_mounts,
        redirections=redirections,
    )


def is_empty_dir(path: Path) -> bool:
    entries = os.listdir(path)
    for ent in entries:
        if ent not in (".", ".."):
            return False
    return True


def print_redirection_configs(
    checkout: EdenCheckout, redirs: Iterable[Redirection], use_json
) -> None:
    redirs = sorted(redirs, key=lambda r: r.repo_path)
    data = [r.as_dict(checkout) for r in redirs]

    if use_json:
        print(json.dumps(data))
    else:
        columns = ["repo_path", "type", "target", "source", "state"]
        print(tabulate.tabulate(columns, data))


@redirect_cmd("list", "List redirections")
class ListCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("--mount", help="The eden mount point path.", default=None)
        parser.add_argument(
            "--json",
            help="output in json rather than human readable text",
            action="store_true",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)

        mount_table = mtab.new()
        redirs = get_effective_redirections(checkout, mount_table)
        print_redirection_configs(checkout, redirs.values(), args.json)
        return 0


@redirect_cmd(
    "fixup",
    (
        "Fixup redirection configuration; redirect things that "
        "should be redirected and remove things that should not be redirected"
    ),
)
class FixupCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("--mount", help="The eden mount point path.", default=None)
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

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)
        mount_table = mtab.new()
        redirs = get_effective_redirections(checkout, mount_table)

        for redir in redirs.values():
            if redir.state == RedirectionState.MATCHES_CONFIGURATION and not (
                args.force_remount_bind_mounts
                and redir.type in (RedirectionType.LEGACY, RedirectionType.BIND)
            ):
                continue

            print(f"Fixing {redir.repo_path}", file=sys.stderr)
            redir.remove_existing(checkout)
            if redir.type == RedirectionType.UNKNOWN:
                continue
            redir.apply(checkout)

        # recompute and display the current state
        redirs = get_effective_redirections(checkout, mount_table)
        ok = True
        for redir in redirs.values():
            if redir.state != RedirectionState.MATCHES_CONFIGURATION:
                ok = False
        return 0 if ok else 1


@redirect_cmd("add", "Add or change a redirection")
class AddCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("--mount", help="The eden mount point path.", default=None)
        parser.add_argument(
            "repo_path", help="The path in the repo which should be redirected"
        )
        parser.add_argument(
            "redir_type",
            help="The type of the redirection",
            choices=["bind", "symlink"],
        )

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
        redir = Redirection(args.repo_path, redir_type, None, USER_REDIRECTION_SOURCE)

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
        parser.add_argument("--mount", help="The eden mount point path.", default=None)
        parser.add_argument(
            "repo_path",
            help="The path in the repo which should no longer be redirected",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)

        redirs = get_configured_redirections(checkout)
        redir = redirs.get(args.repo_path)
        if redir:
            redir.remove_existing(checkout)
            del redirs[args.repo_path]
            config = apply_redirection_configs_to_checkout_config(
                checkout, redirs.values()
            )
            checkout.save_config(config)
            return 0

        redirs = get_effective_redirections(checkout, mtab.new())
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

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        self.add_subcommands(parser, redirect_cmd.commands)

    def run(self, args: argparse.Namespace) -> int:
        # FIXME: I'd rather just show the help here automatically
        print("Specify a subcommand! See `eden redirect --help`", file=sys.stderr)
        return 1
