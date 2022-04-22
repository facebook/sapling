# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

from pathlib import Path
from typing import Union

from .file import File

PathLike = Union[File, Path, str]
