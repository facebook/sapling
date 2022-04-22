# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from .hg import hg
from .repo import Repo
from .util import new_dir


class Server:
    url: str

    def __init__(self) -> None:
        # Satisfy pyre
        self.url = ""

    def clone(self) -> Repo:
        root = new_dir()
        hg(root).clone(self.url, root, noupdate=True)
        return Repo(root)


class LocalServer(Server):
    """An EagerRepo backed EdenApi server."""

    def __init__(self) -> None:
        dir = new_dir()
        self.url = f"eager://{dir}"
