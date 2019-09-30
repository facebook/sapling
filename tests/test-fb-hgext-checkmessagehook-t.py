# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
checkmessagehook=
""" >> "$HGRCPATH"

# Build up a repo

sh % "hg init repo"
sh % "cd repo"
sh % "touch a"
sh % 'hg commit -A -l "$TESTDIR/ctrlchar-msg.txt"' == r"""
    adding a
    non-printable characters in commit message
    Line 5: 'This has a sneaky ctrl-A: \x01'
    Line 6: 'And this has esc: \x1b'
    transaction abort!
    rollback completed
    abort: pretxncommit.checkmessage hook failed
    [255]"""
sh % 'hg commit -A -l "$TESTDIR/perfectlyok-msg.txt"' == "adding a"
sh % "hg log -r ." == r"""
    changeset:   0:d9cf9881be7b
    tag:         tip
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     This commit message is perfectly OK, and has no sneaky control characters."""
