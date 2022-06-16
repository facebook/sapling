#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import binascii
import os
import shutil
import stat
import sys
import typing
import uuid
from pathlib import Path
from typing import Dict, Iterable, List, NamedTuple, Optional, Tuple, Union

import eden.dirstate
import facebook.eden.ttypes as eden_ttypes
from eden.fs.cli import mtab, version as version_mod
from eden.fs.cli.config import (
    CheckoutConfig,
    EdenCheckout,
    EdenInstance,
    HealthStatus,
    ListMountInfo,
)
from fb303_core.ttypes import fb303_status

from .fake_client import FakeClient
from .fake_hg_repo import FakeHgRepo
from .fake_mount_table import FakeMountTable


class FakeCheckout(NamedTuple):
    state_dir: Path
    config: CheckoutConfig
    snapshot: str


class FakeEdenInstance:
    default_commit_hash: str = "1" * 40
    _build_info: Dict[str, str] = {}

    def __init__(
        self,
        tmp_dir: str,
        status: fb303_status = fb303_status.ALIVE,
        build_info: Optional[Dict[str, str]] = None,
        config: Optional[Dict[str, str]] = None,
    ) -> None:
        self._tmp_dir = tmp_dir
        self._status = status
        self._build_info = build_info if build_info else {}
        self._config = config if config else {}
        self._fake_client = FakeClient()

        self._eden_dir = Path(self._tmp_dir) / "eden"
        self._eden_dir.mkdir()
        self.clients_path = self._eden_dir / "clients"
        self.clients_path.mkdir()
        self.default_backing_repo = Path(self._tmp_dir) / "eden-repos" / "main_repo"
        (self.default_backing_repo / ".hg").mkdir(parents=True)

        # A map from mount path --> FakeCheckout
        self._checkouts_by_path: Dict[str, FakeCheckout] = {}
        self._hg_repo_by_path: Dict[Path, FakeHgRepo] = {}
        self.mount_table = FakeMountTable()
        self._next_dev_id = 10

    @property
    def state_dir(self) -> Path:
        return self._eden_dir

    def create_test_mount(
        self,
        path: str,
        snapshot: Optional[str] = None,
        client_name: Optional[str] = None,
        scm_type: str = "hg",
        active: bool = True,
        setup_path: bool = True,
        dirstate_parent: Union[str, Tuple[str, str], None] = None,
        backing_repo: Optional[Path] = None,
    ) -> EdenCheckout:
        """
        Define a configured mount.

        If active is True and status was set to ALIVE when creating the FakeClient
        then the mount will appear as a normal active mount.  It will be reported in the
        thrift results and the mount table, and the mount directory will be populated
        with a .hg/ or .git/ subdirectory.

        The setup_path argument can be set to False to prevent creating the fake mount
        directory on disk.

        Returns the absolute path to the mount directory.
        """
        full_path = os.path.join(self._tmp_dir, path)
        if full_path in self._checkouts_by_path:
            raise Exception(f"duplicate mount definition: {full_path}")

        if snapshot is None:
            snapshot = self.default_commit_hash
        if client_name is None:
            client_name = path.replace("/", "_")
        backing_repo_path = (
            backing_repo if backing_repo is not None else self.default_backing_repo
        )

        state_dir = self.clients_path / client_name
        assert full_path not in self._checkouts_by_path
        config = CheckoutConfig(
            backing_repo=backing_repo_path,
            scm_type=scm_type,
            # pyre-fixme[6]: Expected `str` for 3rd param but got `UUID`.
            guid=uuid.uuid4(),
            mount_protocol="prjfs" if sys.platform == "win32" else "fuse",
            case_sensitive=sys.platform == "linux",
            require_utf8_path=True,
            default_revision=snapshot,
            redirections={},
            active_prefetch_profiles=[],
            predictive_prefetch_profiles_active=False,
            predictive_prefetch_num_dirs=0,
            enable_tree_overlay=True,
            use_write_back_cache=False,
        )
        checkout = FakeCheckout(state_dir=state_dir, config=config, snapshot=snapshot)
        self._checkouts_by_path[full_path] = checkout

        # Write out the config file and snapshot file
        state_dir.mkdir()
        eden_checkout = EdenCheckout(
            typing.cast(EdenInstance, self), Path(full_path), state_dir
        )
        eden_checkout.save_config(config)
        eden_checkout.save_snapshot(snapshot)

        if active and self._status == fb303_status.ALIVE:
            # Report the mount in /proc/mounts
            dev_id = self._next_dev_id
            self._next_dev_id += 1
            self.mount_table.stats[full_path] = mtab.MTStat(
                st_uid=os.getuid(), st_dev=dev_id, st_mode=(stat.S_IFDIR | 0o755)
            )

            # Tell the thrift client to report the mount as active
            self._fake_client._mounts.append(
                eden_ttypes.MountInfo(
                    mountPoint=os.fsencode(full_path),
                    edenClientPath=os.fsencode(state_dir),
                    state=eden_ttypes.MountState.RUNNING,
                )
            )

            # Set up directories on disk that look like the mounted checkout
            if setup_path:
                os.makedirs(full_path)
                if scm_type == "hg":
                    self._setup_hg_path(full_path, checkout, dirstate_parent)
                elif scm_type == "git":
                    os.mkdir(os.path.join(full_path, ".git"))

        return EdenCheckout(
            typing.cast(EdenInstance, self), Path(full_path), Path(state_dir)
        )

    def remove_checkout_configuration(self, mount_path: str) -> None:
        """Update the state to make it look like the specified mount path is still
        actively mounted but not configured on disk."""
        checkout = self._checkouts_by_path.pop(mount_path)
        shutil.rmtree(checkout.state_dir)

    def _setup_hg_path(
        self,
        full_path: str,
        fake_checkout: FakeCheckout,
        dirstate_parent: Union[str, Tuple[str, str], None],
    ) -> None:
        hg_dir = Path(full_path) / ".hg"
        hg_dir.mkdir()
        dirstate_path = hg_dir / "dirstate"

        if dirstate_parent is None:
            # The dirstate parent should normally match the snapshot hash
            parents = (binascii.unhexlify(fake_checkout.snapshot), b"\x00" * 20)
        elif isinstance(dirstate_parent, str):
            # Assume we were given a single parent hash as a hex string
            parents = (binascii.unhexlify(dirstate_parent), b"\x00" * 20)
        else:
            # Assume we were given a both parent hashes as hex strings
            parents = (
                binascii.unhexlify(dirstate_parent[0]),
                binascii.unhexlify(dirstate_parent[1]),
            )

        with dirstate_path.open("wb") as f:
            eden.dirstate.write(f, parents, tuples_dict={}, copymap={})

        (hg_dir / "hgrc").write_text("# This file simply needs to exist\n")
        (hg_dir / "requires").write_text("eden\nremotefilelog\nrevlogv1\nstore\n")
        (hg_dir / "sharedpath").write_bytes(
            bytes(fake_checkout.config.backing_repo / ".hg")
        )
        (hg_dir / "shared").write_text("bookmarks\n")
        (hg_dir / "bookmarks").touch()
        (hg_dir / "branch").write_text("default\n")

        source = str(fake_checkout.config.backing_repo)
        fake_repo = FakeHgRepo(source)
        self._hg_repo_by_path[Path(full_path)] = fake_repo
        self._hg_repo_by_path[fake_checkout.config.backing_repo] = fake_repo

    def get_mount_paths(self) -> Iterable[str]:
        return self._checkouts_by_path.keys()

    # TODO: Improve this mock. The real get_mounts() requests info from thrift.
    def get_mounts(self) -> Dict[Path, ListMountInfo]:
        mount_points: Dict[Path, ListMountInfo] = {}
        for strPath, checkout in self._checkouts_by_path.items():
            path = Path(strPath)
            data_dir = checkout.state_dir
            # For mocking purposes, this is being filled in with some default values
            # If you want an actual implementation of get_mounts(), You will need to
            # rewrite this function to determine actual values for ListMountInfo
            mount_points[path] = ListMountInfo(
                path=path,
                data_dir=data_dir,
                state=None,
                configured=True,
                backing_repo=None,
            )
        return mount_points

    def mount(self, path: str, read_only: bool) -> int:
        assert self._status in (
            fb303_status.ALIVE,
            fb303_status.STARTING,
            fb303_status.STOPPING,
        )
        assert path in self._checkouts_by_path
        return 0

    def check_health(self) -> HealthStatus:
        return HealthStatus(self._status, pid=None, uptime=None, detail="")

    def check_privhelper_connection(self) -> bool:
        return True

    def get_server_build_info(self) -> Dict[str, str]:
        return dict(self._build_info)

    def get_thrift_client_legacy(self) -> FakeClient:
        return self._fake_client

    def get_checkouts(self) -> List[EdenCheckout]:
        results: List[EdenCheckout] = []
        for mount_path, checkout in self._checkouts_by_path.items():
            results.append(
                EdenCheckout(
                    typing.cast(EdenInstance, self),
                    Path(mount_path),
                    Path(checkout.state_dir),
                )
            )
        return results

    def get_config_value(self, key: str, default: str) -> str:
        return self._config.get(key, default)

    def get_hg_repo(self, path: Path) -> FakeHgRepo:
        if path in self._hg_repo_by_path:
            return self._hg_repo_by_path[path]

        def bad_commit_checker(commit: str) -> bool:
            return False

        fake_repo = FakeHgRepo(str(path))
        fake_repo.commit_checker = bad_commit_checker
        return fake_repo

    def log_sample(self, log_type: str, **kwargs: Union[bool, int, str, float]) -> None:
        pass

    def get_running_version_parts(self) -> Tuple[str, str]:
        return (
            self._build_info.get("build_package_version", ""),
            self._build_info.get("build_package_release", ""),
        )

    def get_running_version(self) -> str:
        return version_mod.format_eden_version(self.get_running_version_parts())
