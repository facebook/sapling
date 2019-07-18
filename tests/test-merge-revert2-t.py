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
sh % "echo 'another line of text'" >> "file1"
sh % "echo 'added file2'" > "file2"
sh % "hg add file1 file2"
sh % "hg commit -m 'added file1 and file2'"

sh % "echo 'changed file1'" >> "file1"
sh % "hg commit -m 'changed file1'"

sh % "hg -q log" == r"""
    1:dfab7f3c2efb
    0:c3fa057dd86f"""
sh % "hg id" == "dfab7f3c2efb tip"

sh % "hg update -C 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg id" == "c3fa057dd86f"

sh % "echo 'changed file1'" >> "file1"
sh % "hg id" == "c3fa057dd86f+"

sh % "hg revert --no-backup --all" == "reverting file1"
sh % "hg diff"
sh % "hg status"
sh % "hg id" == "c3fa057dd86f"

sh % "hg update" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg diff"
sh % "hg status"
sh % "hg id" == "dfab7f3c2efb tip"

sh % "hg update -C 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo 'changed file1 different'" >> "file1"

sh % "hg update" == r"""
    merging file1
    warning: 1 conflicts while merging file1! (edit, then use 'hg resolve --mark')
    0 files updated, 0 files merged, 0 files removed, 1 files unresolved
    use 'hg resolve' to retry unresolved file merges
    [1]"""

sh % "hg diff --nodates" == r"""
    diff -r dfab7f3c2efb file1
    --- a/file1
    +++ b/file1
    @@ -1,3 +1,7 @@
     added file1
     another line of text
    +<<<<<<< working copy: c3fa057dd86f - test: added file1 and file2
    +changed file1 different
    +=======
     changed file1
    +>>>>>>> destination:  dfab7f3c2efb - test: changed file1"""

sh % "hg status" == r"""
    M file1
    ? file1.orig"""
sh % "hg id" == "dfab7f3c2efb+ tip"

sh % "hg revert --no-backup --all" == "reverting file1"
sh % "hg diff"
sh % "hg status" == "? file1.orig"
sh % "hg id" == "dfab7f3c2efb tip"

sh % "hg revert -r tip --no-backup --all"
sh % "hg diff"
sh % "hg status" == "? file1.orig"
sh % "hg id" == "dfab7f3c2efb tip"

sh % "hg update -C" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg diff"
sh % "hg status" == "? file1.orig"
sh % "hg id" == "dfab7f3c2efb tip"
