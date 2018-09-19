#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc
import contextlib
import json
import logging
import subprocess
import tempfile
import time
import types
from pathlib import Path
from typing import Callable, Dict, Iterator, List, Optional, Type, TypeVar, Union

from eden.integration.lib import edenclient, hgrepo, util


T = TypeVar("T", bound="BaseSnapshot")


class BaseSnapshot:
    # The NAME and DESCRIPTION class fields are intended to be overridden on subclasses
    # by the @snapshot_class decorator.
    NAME = "Base Snapshot Class"
    DESCRIPTION = ""

    def __init__(self, base_dir: Path) -> None:
        self.base_dir = base_dir
        self.eden: Optional[edenclient.EdenFS] = None

    def __enter__(self: T) -> T:
        return self

    def __exit__(
        self,
        exc_type: Optional[Type[BaseException]],
        exc_value: Optional[BaseException],
        tb: Optional[types.TracebackType],
    ) -> None:
        self.cleanup()

    def cleanup(self) -> None:
        if self.eden is not None:
            try:
                self.eden.kill()
            except Exception as ex:
                logging.exception("error stopping edenfs")
            self.eden = None

    def create_tarball(self, output_path: Path) -> None:
        """Create a tarball from the snapshot contents.

        Note that in most cases you will likely want to save the snapshot state when
        edenfs is not running, to ensure that the snapshot data is in a consistent
        state.
        """
        # Make sure the output directory exists
        output_path.parent.mkdir(parents=True, exist_ok=True)

        cmd = [
            "gtar",
            "-c",
            "--auto-compress",
            "--sort=name",
            # The inode metadata table usually ends with quite a few empty pages.
            # The --sparse flag allows tar to detect these and avoid emitting them.
            # Given that we normally compress the result this doesn't really make
            # much difference on the final compressed size, though.
            "--sparse",
            # Suppress warnings about the fact that tar skips Eden's socket files.
            "--warning=no-file-ignored",
            # The owner and group IDs in the tar file don't really matter.
            # Just record a fixed data rather than pulling them from the
            # current system being used to generate the archive.
            "--owner=nobody:65534",
            "--group=nobody:65534",
        ] + ["-f", str(output_path), "data"]
        subprocess.check_call(cmd, cwd=self.base_dir)

    def generate(self) -> None:
        self._setup_directories()
        self._emit_metadata()
        self.gen_before_eden_running()

        self.eden = edenclient.EdenFS(
            eden_dir=str(self.eden_state_dir),
            etc_eden_dir=str(self.etc_eden_dir),
            home_dir=str(self.home_dir),
            storage_engine="rocksdb",
        )
        try:
            self.eden.start()
            self.gen_eden_running()
        finally:
            self.eden.kill()
            self.eden = None

        self.gen_after_eden_stopped()

    def _setup_directories(self) -> None:
        self.data_dir = self.base_dir / "data"
        self.data_dir.mkdir()

        self.eden_state_dir = self.data_dir / "eden"
        self.etc_eden_dir = self.data_dir / "etc_eden"
        self.etc_eden_dir.mkdir()
        self.home_dir = self.data_dir / "home"
        self.home_dir.mkdir()

    def _emit_metadata(self) -> None:
        data = {
            "type": self.NAME,
            "description": self.DESCRIPTION,
            "time_created": time.time(),
        }

        metadata_path = self.data_dir / "info.json"
        with metadata_path.open("w") as f:
            json.dump(data, f, indent=2, sort_keys=True)

    def gen_before_eden_running(self) -> None:
        """gen_before_eden_running() will be called when generating a new snapshot after
        the directory structure has been set up but before edenfs is started.

        Subclasses of BaseSnapshot can perform any work they want here.
        """
        pass

    def gen_eden_running(self) -> None:
        """gen_eden_running() will be called when generating a new snapshot once edenfs
        has been started.

        Subclasses of BaseSnapshot can perform any work they want here.
        """
        pass

    def gen_after_eden_stopped(self) -> None:
        """gen_after_eden_stopped() will be called as the final step of generating a
        snapshot, once edenfs has been stopped.

        Subclasses of BaseSnapshot can perform any work they want here.
        """
        pass


class HgSnapshot(BaseSnapshot, metaclass=abc.ABCMeta):
    """A helper parent class for BaseSnapshot implementations that creates a single
    checkout of a mercurial repository."""

    def gen_before_eden_running(self) -> None:
        # Prepare the system hgrc file
        self.system_hgrc_path = self.data_dir / "system_hgrc"
        self.system_hgrc_path.write_text(hgrepo.HgRepository.get_system_hgrc_contents())

        logging.info("Creating backing repository...")
        # Create the repository
        backing_repo_path = self.data_dir / "repo"
        backing_repo_path.mkdir()
        self.backing_repo = hgrepo.HgRepository(
            str(backing_repo_path), system_hgrc=str(self.system_hgrc_path)
        )
        self.backing_repo.init()

        self.populate_backing_repo()

    def gen_eden_running(self) -> None:
        assert self.eden is not None
        logging.info("Preparing checkout...")

        checkout_path = self.data_dir / "checkout"
        self.eden.clone(self.backing_repo.path, str(checkout_path))

        self.checkout_repo = hgrepo.HgRepository(
            str(checkout_path), system_hgrc=str(self.system_hgrc_path)
        )
        self.populate_checkout()

    @abc.abstractmethod
    def populate_backing_repo(self) -> None:
        pass

    @abc.abstractmethod
    def populate_checkout(self) -> None:
        pass

    def checkout_path(self, *args: Union[Path, str]) -> Path:
        """Compute a path inside the checkout."""
        return Path(self.checkout_repo.path, *args)

    def read_file(self, path: Union[Path, str]) -> bytes:
        """Helper function to read a file in the checkout.
        This is primarily used to ensure that the file is loaded.
        """
        file_path = self.checkout_path(path)
        with file_path.open("rb") as f:
            data: bytes = f.read()
        return data

    def write_file(self, path: Union[Path, str], contents: bytes) -> None:
        """Helper function to write a file in the checkout."""
        file_path = self.checkout_path(path)
        with file_path.open("wb") as f:
            f.write(contents)

    def list_dir(self, path: Union[Path, str]) -> List[Path]:
        """List the contents of a directory in the checkout.
        This can be used to ensure the directory has been loaded by Eden.
        """
        dir_path = self.checkout_path(path)
        return list(dir_path.iterdir())


snapshot_types: Dict[str, Type[BaseSnapshot]] = {}


def snapshot_class(
    name: str, description: str
) -> Callable[[Type[BaseSnapshot]], Type[BaseSnapshot]]:
    """A decorator for registering snapshot implementations."""

    def wrapper(snapshot: Type[BaseSnapshot]) -> Type[BaseSnapshot]:
        snapshot.NAME = name
        snapshot.DESCRIPTION = description
        snapshot_types[name] = snapshot
        return snapshot

    return wrapper


@contextlib.contextmanager
def create_tmp_dir() -> Iterator[Path]:
    """A helper class to manage temporary directories for snapshots.

    This is similar to the standard tempdir.TemporaryDirectory code,
    but does a better job of cleaning up the directory if some of its contents are
    read-only.
    """
    tmpdir = Path(tempfile.mkdtemp(prefix="eden_data."))
    try:
        yield tmpdir
    finally:
        util.cleanup_tmp_dir(tmpdir)


@contextlib.contextmanager
def generate(snapshot_type: Type[T]) -> Iterator[T]:
    """Generate a snapshot using the specified snapshot type.

    The argument must be a subclass of BaseSnapshot.
    This should be used in a `with` statement.  This method generates the snapshot in a
    temporary directory that will be cleaned up when exiting the `with` context.
    """
    with create_tmp_dir() as tmpdir:
        with snapshot_type(tmpdir) as snapshot:
            snapshot.generate()
            yield snapshot


def _import_snapshot_modules() -> None:
    import __manifest__

    # Find and import all modules in our "types" sub-package.
    # Each module will register its snapshot types when imported.
    package_prefix = f"{__package__}.types."
    for module in __manifest__.modules:  # type: ignore
        if module.startswith(package_prefix):
            __import__(module)


# Automatically import all snapshot modules to register their snapshot classes
_import_snapshot_modules()
