#debugruntest-compatible
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > amend=
  > undo =
  > [experimental]
  > evolution = obsolete
  > [mutation]
  > enabled = true
  > [visibility]
  > enabled = true
  > EOF

# Create repo

  $ hg init repo
  $ cd repo
  $ drawdag << 'EOS'
  > E
  > |
  > C D
  > |/
  > B
  > |
  > A
  > EOS

  $ hg book -r $C cat
  $ hg book -r $B dog
  $ hg goto $A
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  4 E
  │
  │ o  3 D
  │ │
  o │  2 C cat
  ├─╯
  o  1 B dog
  │
  @  0 A

# Hide a single commit

  $ hg hide $D
  hiding commit be0ef73c17ad "D"
  1 changeset hidden
  hint[undo]: you can undo this using the `hg undo` command
  hint[hint-ack]: use 'hg hint --ack undo' to silence these hints
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  4 E
  │
  o  2 C cat
  │
  o  1 B dog
  │
  @  0 A

# Hide multiple commits with bookmarks on them, hide wc parent

  $ hg goto $B
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg hide .
  hiding commit 112478962961 "B"
  hiding commit 26805aba1e60 "C"
  hiding commit 78d2dca436b2 "E"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 426bada5c675
  3 changesets hidden
  removing bookmark 'cat' (was at: 26805aba1e60)
  removing bookmark 'dog' (was at: 112478962961)
  2 bookmarks removed
  hint[undo]: you can undo this using the `hg undo` command
  hint[hint-ack]: use 'hg hint --ack undo' to silence these hints
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  @  0 A

# Unhide stuff

  $ hg unhide 2
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  2 C
  │
  o  1 B
  │
  @  0 A
  $ hg unhide -r 4 -r 3
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  4 E
  │
  │ o  3 D
  │ │
  o │  2 C
  ├─╯
  o  1 B
  │
  @  0 A

# hg hide --cleanup tests

  $ hg goto 4
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo f > f
  $ hg add f
  $ hg commit -d '0 0' -m F
  $ hg goto 4
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg amend --no-rebase -m E2 -d '0 0'
  hint[amend-restack]: descendants of 78d2dca436b2 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  @  6 E2
  │
  │ o  5 F
  │ │
  │ x  4 E
  ├─╯
  │ o  3 D
  │ │
  o │  2 C
  ├─╯
  o  1 B
  │
  o  0 A
  $ hg hide -c
  abort: nothing to hide
  [255]
  $ hg hide -c -r .
  abort: --rev and --cleanup are incompatible
  [255]
  $ hg --config 'extensions.rebase=' rebase -s 5 -d 6
  rebasing 1f7934a9b4de "F"
  $ hg book -r 5 alive --hidden
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  7 F
  │
  @  6 E2
  │
  │ x  5 F alive
  │ │
  │ x  4 E
  ├─╯
  │ o  3 D
  │ │
  o │  2 C
  ├─╯
  o  1 B
  │
  o  0 A
  $ hg hide --cleanup
  hiding commit 78d2dca436b2 "E"
  hiding commit 1f7934a9b4de "F"
  2 changesets hidden
  removing bookmark 'alive' (was at: 1f7934a9b4de)
  1 bookmark removed
  hint[undo]: you can undo this using the `hg undo` command
  hint[hint-ack]: use 'hg hint --ack undo' to silence these hints
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  7 F
  │
  @  6 E2
  │
  │ o  3 D
  │ │
  o │  2 C
  ├─╯
  o  1 B
  │
  o  0 A

# Hiding the head bookmark of a stack hides the stack.

  $ hg book -r 3 somebookmark
  $ hg hide -B somebookmark
  hiding commit be0ef73c17ad "D"
  1 changeset hidden
  removing bookmark 'somebookmark' (was at: be0ef73c17ad)
  1 bookmark removed
  hint[undo]: you can undo this using the `hg undo` command
  hint[hint-ack]: use 'hg hint --ack undo' to silence these hints
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  7 F
  │
  @  6 E2
  │
  o  2 C
  │
  o  1 B
  │
  o  0 A

# Hiding a bookmark in the middle of a stack just deletes the bookmark.

  $ hg book -r 2 stackmidbookmark
  $ hg hide -B stackmidbookmark
  removing bookmark 'stackmidbookmark' (was at: 26805aba1e60)
  1 bookmark removed
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  7 F
  │
  @  6 E2
  │
  o  2 C
  │
  o  1 B
  │
  o  0 A
