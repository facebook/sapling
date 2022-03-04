# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import sh, testtmp  # noqa: F401


(
    sh % "cat"
    << r"""
[extensions]
checkmessagehook=
"""
    >> "$HGRCPATH"
)

# Build up a repo

sh % "hg init repo" == ""
sh % "cd repo"
sh % "touch a"
sh % 'hg commit -A -l "$TESTDIR/ctrlchar-msg.txt"' == r"""
    adding a
    +-------------------------------------------------------------
    | Non-printable characters in commit message are not allowed.
    | Edit your commit message to fix this issue.
    | The problematic commit message can be found at:
    |  Line 5: 'This has a sneaky ctrl-A: \x01'
    |  Line 6: 'And this has esc: \x1b'
    +-------------------------------------------------------------
    abort: pretxncommit.checkmessage hook failed
    [255]"""
sh % 'hg commit -A -l "$TESTDIR/perfectlyok-msg.txt"' == "adding a"
sh % "hg log -r ." == r"""
    commit:      d9cf9881be7b
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     This commit message is perfectly OK, and has no sneaky control characters."""

# Try force adding a non-printable character
sh % "touch b"
sh % 'hg commit -A -l "$TESTDIR/ctrlchar-msg.txt" --config checkmessage.allownonprintable=True' == "adding b"
