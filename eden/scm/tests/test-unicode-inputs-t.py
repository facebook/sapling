# coding=utf-8
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os

from testutil.autofix import eq
from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setmodernconfig"

sh % "hg init repo"
sh % "cd repo"
sh % "echo xxx" > "file"
sh % "hg add file"
sh % "hg commit -m 'Æ'"

sh % "hg log -v" == """
commit:      4bb70d3b3100
user:        test
date:        Thu Jan 01 00:00:00 1970 +0000
files:       file
description:
Æ
"""
