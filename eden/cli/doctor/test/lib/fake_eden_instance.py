#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import binascii
import collections
import os
import shutil
import stat
import typing
from pathlib import Path
from typing import Dict, Iterable, List, NamedTuple, Optional, Tuple, Union

import eden.dirstate
import facebook.eden.ttypes as eden_ttypes
from eden.cli import mtab
from eden.cli.config import CheckoutConfig, EdenCheckout, EdenInstance, HealthStatus
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
        self._hg_repo_by_path: Dict[str, FakeHgRepo] = {}
        self.mount_table = FakeMountTable()
        self._next_dev_id = 10

    @property
    def state_dir(self) -> Path:
        return self._eden_dir

    def create_test_mount(
        self,
        path: str,
        snapshot: Optional[str] = None,
        bind_mounts: Optional[Dict[str, str]] = None,
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
        if bind_mounts is None:
            bind_mounts = {}
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
            bind_mounts=bind_mounts,
            default_revision=snapshot,
            redirections={},
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

        self._hg_repo_by_path[full_path] = FakeHgRepo()

    def get_mount_paths(self) -> Iterable[str]:
        return self._checkouts_by_path.keys()

    def mount(self, path: str) -> int:
        assert self._status in (
            fb303_status.ALIVE,
            fb303_status.STARTING,
            fb303_status.STOPPING,
        )
        assert path in self._checkouts_by_path
        return 0

    def check_health(self) -> HealthStatus:
        return HealthStatus(self._status, pid=None, detail="")

    def get_client_info(self, mount_path: str) -> collections.OrderedDict:
        checkout = self._checkouts_by_path[mount_path]
        return collections.OrderedDict(
            [
                ("bind-mounts", checkout.config.bind_mounts),
                ("mount", mount_path),
                ("scm_type", checkout.config.scm_type),
                ("snapshot", checkout.snapshot),
                ("client-dir", checkout.state_dir),
            ]
        )

    def get_server_build_info(self) -> Dict[str, str]:
        return dict(self._build_info)

    def get_thrift_client(self) -> FakeClient:
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

    def get_hg_repo(self, path: str) -> Optional[FakeHgRepo]:
        if path in self._hg_repo_by_path:
            return self._hg_repo_by_path[path]
        return None
