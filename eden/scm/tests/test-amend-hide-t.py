# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
amend=
undo =
[experimental]
evolution = createmarkers, allowunstable
""" >> "$HGRCPATH"

# Create repo
sh % "hg init"
sh % "hg debugdrawdag" << r"""
E
|
C D
|/
B
|
A
"""
sh % "rm .hg/localtags"

sh % "hg book -r 2 cat"
sh % "hg book -r 1 dog"
sh % "hg update 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

sh % "hg log -G -T '{rev} {desc} {bookmarks}\\n'" == r"""
    o  4 E
    |
    | o  3 D
    | |
    o |  2 C cat
    |/
    o  1 B dog
    |
    @  0 A"""

# Hide a single commit
sh % "hg hide 3" == r"""
    hiding commit be0ef73c17ad "D"
    1 changeset hidden
    hint[undo]: you can undo this using the `hg undo` command
    hint[hint-ack]: use 'hg hint --ack undo' to silence these hints"""
sh % "hg log -G -T '{rev} {desc} {bookmarks}\\n'" == r"""
    o  4 E
    |
    o  2 C cat
    |
    o  1 B dog
    |
    @  0 A"""

# Hide multiple commits with bookmarks on them, hide wc parent
sh % "hg update 1" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg hide ." == r"""
    hiding commit 112478962961 "B"
    hiding commit 26805aba1e60 "C"
    hiding commit 78d2dca436b2 "E"
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved
    working directory now at 426bada5c675
    3 changesets hidden
    removing bookmark "cat (was at: 26805aba1e60)"
    removing bookmark "dog (was at: 112478962961)"
    2 bookmarks removed
    hint[undo]: you can undo this using the `hg undo` command
    hint[hint-ack]: use 'hg hint --ack undo' to silence these hints"""
sh % "hg log -G -T '{rev} {desc} {bookmarks}\\n'" == "@  0 A"

# Unhide stuff
sh % "hg unhide 2"
sh % "hg log -G -T '{rev} {desc} {bookmarks}\\n'" == r"""
    o  2 C
    |
    o  1 B
    |
    @  0 A"""
sh % "hg unhide -r 4 -r 3"
sh % "hg log -G -T '{rev} {desc} {bookmarks}\\n'" == r"""
    o  4 E
    |
    | o  3 D
    | |
    o |  2 C
    |/
    o  1 B
    |
    @  0 A"""

# hg hide --cleanup tests
sh % "hg update 4" == "3 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo f" > "f"
sh % "hg add f"
sh % "hg commit -d '0 0' -m F"
sh % "hg update 4" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "hg amend --no-rebase -m E2 -d '0 0'" == r"""
    hint[amend-restack]: descendants of 78d2dca436b2 are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg log -G -T '{rev} {desc} {bookmarks}\\n'" == r"""
    @  6 E2
    |
    | o  5 F
    | |
    | x  4 E
    |/
    | o  3 D
    | |
    o |  2 C
    |/
    o  1 B
    |
    o  0 A"""
sh % "hg hide -c" == r"""
    abort: nothing to hide
    [255]"""
sh % "hg hide -c -r ." == r"""
    abort: --rev and --cleanup are incompatible
    [255]"""
sh % "hg --config 'extensions.rebase=' rebase -s 5 -d 6" == 'rebasing 1f7934a9b4de "F"'
sh % "hg book -r 5 alive --hidden"
sh % "hg log -G -T '{rev} {desc} {bookmarks}\\n'" == r"""
    o  7 F
    |
    @  6 E2
    |
    | x  5 F alive
    | |
    | x  4 E
    |/
    | o  3 D
    | |
    o |  2 C
    |/
    o  1 B
    |
    o  0 A"""
sh % "hg hide --cleanup" == r"""
    hiding commit 78d2dca436b2 "E"
    hiding commit 1f7934a9b4de "F"
    2 changesets hidden
    removing bookmark "alive (was at: 1f7934a9b4de)"
    1 bookmark removed
    hint[undo]: you can undo this using the `hg undo` command
    hint[hint-ack]: use 'hg hint --ack undo' to silence these hints"""
sh % "hg log -G -T '{rev} {desc} {bookmarks}\\n'" == r"""
    o  7 F
    |
    @  6 E2
    |
    | o  3 D
    | |
    o |  2 C
    |/
    o  1 B
    |
    o  0 A"""
# Hiding the head bookmark of a stack hides the stack.
sh % "hg book -r 3 somebookmark"
sh % "hg hide -B somebookmark" == r"""
    hiding commit be0ef73c17ad "D"
    1 changeset hidden
    removing bookmark "somebookmark (was at: be0ef73c17ad)"
    1 bookmark removed
    hint[undo]: you can undo this using the `hg undo` command
    hint[hint-ack]: use 'hg hint --ack undo' to silence these hints"""
sh % "hg log -G -T '{rev} {desc} {bookmarks}\\n'" == r"""
    o  7 F
    |
    @  6 E2
    |
    o  2 C
    |
    o  1 B
    |
    o  0 A"""
# Hiding a bookmark in the middle of a stack just deletes the bookmark.
sh % "hg book -r 2 stackmidbookmark"
sh % "hg hide -B stackmidbookmark" == r"""
    removing bookmark 'stackmidbookmark' (was at: 26805aba1e60)
    1 bookmark removed"""
sh % "hg log -G -T '{rev} {desc} {bookmarks}\\n'" == r"""
    o  7 F
    |
    @  6 E2
    |
    o  2 C
    |
    o  1 B
    |
    o  0 A"""
