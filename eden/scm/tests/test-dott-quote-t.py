# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os

from testutil.dott import sh, testtmp  # noqa: F401


sh % "echo 1" > "B1"
sh % "echo 2" > "B2"

os.environ["X"] = "1"

#         | no quote  | ' quote | " quote
# environ | expand    | as-is   | expand
# globs   | expand    | as-is   | as-is
sh % r"""echo A B* 'B*' "B*" C $X '$X' "$X" Z""" == r"""
    A B1 B2 B* B* C 1 $X 1 Z
"""
