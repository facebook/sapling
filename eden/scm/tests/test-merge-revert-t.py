# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". helpers-usechg.sh"

sh % "hg init"

sh % "echo 'added file1'" > "file1"
sh % "echo 'added file2'" > "file2"
sh % "hg add file1 file2"
sh % "hg commit -m 'added file1 and file2'"

sh % "echo 'changed file1'" >> "file1"
sh % "hg commit -m 'changed file1'"

sh % "hg -q log" == r"""
    1:08a16e8e4408
    0:d29c767a4b52"""
sh % "hg id" == "08a16e8e4408"

sh % "hg update -C 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg id" == "d29c767a4b52"
sh % "echo 'changed file1'" >> "file1"
sh % "hg id" == "d29c767a4b52+"

sh % "hg revert --all" == "reverting file1"
sh % "hg diff"
sh % "hg status" == "? file1.orig"
sh % "hg id" == "d29c767a4b52"

sh % "hg update" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg diff"
sh % "hg status" == "? file1.orig"
sh % "hg id" == "08a16e8e4408"

sh % "hg update -C 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo 'changed file1'" >> "file1"

sh % "hg update" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg diff"
sh % "hg status" == "? file1.orig"
sh % "hg id" == "08a16e8e4408"

sh % "hg revert --all"
sh % "hg diff"
sh % "hg status" == "? file1.orig"
sh % "hg id" == "08a16e8e4408"

sh % "hg revert -r tip --all"
sh % "hg diff"
sh % "hg status" == "? file1.orig"
sh % "hg id" == "08a16e8e4408"

sh % "hg update -C" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg diff"
sh % "hg status" == "? file1.orig"
sh % "hg id" == "08a16e8e4408"
