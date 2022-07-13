# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

import os
from pathlib import Path
from typing import Any, Dict, Generator, IO, List, Optional, TYPE_CHECKING, Union

from .commit import Commit
from .config import Config
from .file import File
from .hg import hg
from .status import Status
from .types import PathLike
from .util import new_dir, new_file, override_environ, test_globals

if TYPE_CHECKING:
    from eden.integration.lib import edenclient

    from .repo import Repo


class WorkingCopy:
    repo: Repo
    root: Path
    hg: hg

    def __init__(self, repo: Repo, root: Path) -> None:
        self.repo = repo
        self.root = root
        self.hg = hg(self.root)

    def checkout(self, destination: Union[str, Commit], clean: bool = False) -> None:
        self.hg.checkout(destination, clean=clean)

    def status(self) -> Status:
        return Status(self.hg.status(copies=True, template="json").stdout)

    def commit(
        self,
        message: Optional[str] = None,
        files: Optional[List[str]] = None,
        author: Optional[str] = None,
        date: Optional[str] = None,
        addremove: bool = False,
    ) -> Commit:
        default_data = test_globals.repo_gen.gen_commit_data()
        files = files or []
        if message is None:
            message = default_data["message"]
        if date is None:
            date = default_data["date"]
        if author is None:
            author = "Tester Author"

        options = dict(
            message=message,
            date=date,
            addremove=addremove,
            user=author,
        )
        self.hg.commit(*files, **options)
        return self.current_commit()

    def amend(
        self,
        message: Optional[str] = None,
        files: Optional[List[str]] = None,
        author: Optional[str] = None,
        date: Optional[str] = None,
        addremove: bool = False,
    ) -> Commit:
        default_data = test_globals.repo_gen.gen_commit_data()

        files = files or []
        if date is None:
            date = default_data["date"]
        if author is None:
            author = "Tester Author"

        options = {
            "message": message,
            "date": date,
            "addremove": addremove,
            "user": author,
        }

        self.hg.amend(*files, **options)
        return self.current_commit()

    def current_commit(self) -> Commit:
        return Commit(self.repo, self.hg.log(rev=".", template="{node}").stdout)

    def file(
        self,
        path: Optional[PathLike] = None,
        content: Optional[Union[bytes, str]] = None,
        add: bool = True,
    ) -> File:
        default_path = test_globals.repo_gen.gen_file_name()
        if path is None:
            path = default_path
        if content is None:
            content = str(path)

        file = self[path]
        file.write(content)

        if add:
            self.add(path)

        return file

    def move(self, source: PathLike, dest: Optional[PathLike] = None) -> File:
        if dest is None:
            dest = str(source) + "_moved"

        self.hg.mv(source, dest)
        return self[dest]

    def backout(
        self, commit: Union[Commit, str], message: Optional[str] = None
    ) -> Commit:
        if message is None:
            message = f"Backout {commit}"
        self.hg.backout(rev=commit, message=message)
        return self.current_commit()

    def __getitem__(self, path: PathLike) -> File:
        return File(self.root, Path(str(path)))

    def add(self, path: PathLike) -> None:
        self.hg.add(str(path))

    def remove(self, path: PathLike) -> None:
        self.hg.remove(str(path), force=True)

    # pyre-ignore[3] - pyre doesn't like that this can return str and bytes
    def open(self, path: PathLike, mode: str = "r") -> IO[Any]:
        return self[path].open(mode)

    def write(self, path: PathLike, content: str) -> None:
        self[path].write(content)

    def join(self, path: PathLike) -> Path:
        # pyre-fixme[7]: Expected `Path` but got `str`.
        return os.path.join(self.root, str(path))

    def files(self) -> List[str]:
        return sorted(self.hg.files().stdout.rstrip().split("\n"))


class EdenWorkingCopy(WorkingCopy):
    eden: edenclient.EdenFS

    def __init__(self, repo: Repo, path: Path) -> None:
        scratch_config = new_file()
        with open(scratch_config, "w+") as f:
            template_dir = str(new_dir()).replace("\\", "\\\\")
            f.write(
                f"""
template = {template_dir}
overrides = {{}}
"""
            )

        overrides = dict(test_globals.env)
        overrides.update(
            {
                "SCRATCH_CONFIG_PATH": str(scratch_config),
                "HG_REAL_BIN": str(hg.EXEC),
            }
        )

        base_dir = new_dir()
        config = Config(Path(test_globals.env["HGRCPATH"]))
        config.add("edenfs", "basepath", str(base_dir))

        from eden.integration.lib import edenclient

        with override_environ(overrides):

            self.eden = edenclient.EdenFS(
                base_dir=base_dir,
                extra_args=["--eden_logview"],
                storage_engine="memory",
            )

            # Write out edenfs config file.
            with open(self.eden.system_rc_path, mode="a+") as eden_rc:
                eden_rc.write(
                    """
[experimental]
enable-nfs-server = true
use-edenapi = true

[hg]
import-batch-size = "32"
import-batch-size-tree = "128"
"""
                )

            self.eden.start()
            self.eden.clone(str(repo.root), str(path), allow_empty=True)

        super().__init__(repo, path)

    def cleanup(self) -> None:
        self.eden.cleanup()
