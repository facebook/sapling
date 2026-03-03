
#require no-eden

# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ eagerepo
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

  $ hg log -G -T '{desc} {bookmarks}\n'
  o  E
  │
  │ o  D
  │ │
  o │  C cat
  ├─╯
  o  B dog
  │
  @  A

# Hide a single commit

  $ hg hide $D
  hiding commit be0ef73c17ad "D"
  1 changeset hidden
  hint[undo]: you can undo this using the `hg undo` command
  hint[hint-ack]: use 'hg hint --ack undo' to silence these hints
  $ hg log -G -T '{desc} {bookmarks}\n'
  o  E
  │
  o  C cat
  │
  o  B dog
  │
  @  A

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
  $ hg log -G -T '{desc} {bookmarks}\n'
  @  A

# Unhide stuff

  $ hg unhide 'desc(C)'
  $ hg log -G -T '{desc} {bookmarks}\n'
  o  C
  │
  o  B
  │
  @  A
  $ hg unhide -r 'desc(E)' -r 'desc(D)'
  $ hg log -G -T '{desc} {bookmarks}\n'
  o  E
  │
  │ o  D
  │ │
  o │  C
  ├─╯
  o  B
  │
  @  A

# hg hide --cleanup tests

  $ hg goto 'desc(E)'
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo f > f
  $ hg add f
  $ hg commit -d '0 0' -m F
  $ hg goto 'desc(E)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg amend --no-rebase -m E2 -d '0 0'
  hint[amend-restack]: descendants of 78d2dca436b2 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg log -G -T '{desc} {bookmarks}\n'
  @  E2
  │
  │ o  F
  │ │
  │ x  E
  ├─╯
  │ o  D
  │ │
  o │  C
  ├─╯
  o  B
  │
  o  A
  $ hg hide -c
  abort: nothing to hide
  [255]
  $ hg hide -c -r .
  abort: --rev and --cleanup are incompatible
  [255]
  $ hg --config 'extensions.rebase=' rebase -s 'desc(F)' -d 'desc(E2)'
  rebasing 1f7934a9b4de "F"
  $ hg book -r 1f7934a9b4de alive --hidden
  $ hg log -G -T '{desc} {bookmarks}\n'
  o  F
  │
  @  E2
  │
  │ x  F alive
  │ │
  │ x  E
  ├─╯
  │ o  D
  │ │
  o │  C
  ├─╯
  o  B
  │
  o  A
  $ hg hide --cleanup
  hiding commit 78d2dca436b2 "E"
  hiding commit 1f7934a9b4de "F"
  2 changesets hidden
  removing bookmark 'alive' (was at: 1f7934a9b4de)
  1 bookmark removed
  hint[undo]: you can undo this using the `hg undo` command
  hint[hint-ack]: use 'hg hint --ack undo' to silence these hints
  $ hg log -G -T '{desc} {bookmarks}\n'
  o  F
  │
  @  E2
  │
  │ o  D
  │ │
  o │  C
  ├─╯
  o  B
  │
  o  A

# Hiding the head bookmark of a stack hides the stack.

  $ hg book -r 'desc(D)' somebookmark
  $ hg hide -B somebookmark
  hiding commit be0ef73c17ad "D"
  1 changeset hidden
  removing bookmark 'somebookmark' (was at: be0ef73c17ad)
  1 bookmark removed
  hint[undo]: you can undo this using the `hg undo` command
  hint[hint-ack]: use 'hg hint --ack undo' to silence these hints
  $ hg log -G -T '{desc} {bookmarks}\n'
  o  F
  │
  @  E2
  │
  o  C
  │
  o  B
  │
  o  A

# Hiding a bookmark in the middle of a stack just deletes the bookmark.

  $ hg book -r 'desc(C)' stackmidbookmark
  $ hg hide -B stackmidbookmark
  removing bookmark 'stackmidbookmark' (was at: 26805aba1e60)
  1 bookmark removed
  $ hg log -G -T '{desc} {bookmarks}\n'
  o  F
  │
  @  E2
  │
  o  C
  │
  o  B
  │
  o  A
