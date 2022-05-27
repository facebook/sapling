#debugruntest-compatible
# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ enable rebase
  $ newrepo
  $ drawdag << 'EOS'
  > D    # A/A=1\n
  > |    # B/A=(removed)
  > B C  # B/Renamed=1\n
  > |/   # C/A=2\n
  > A
  > EOS

  $ hg up -q $C

# rename should support absolute path

  $ ROOT=$(hg root)
  $ hg rebase -r $C -d $D '--config=ui.interactive=1' '--config=experimental.copytrace=off' << EOS
  > r
  > $ROOT/Renamed
  > EOS
  rebasing 85b47c0eb942 "C"
  other [source] changed A which local [dest] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? r
  path 'A' in commit 85b47c0eb942 was renamed to [what path relative to repo root] in commit ed4ad4ec6472 ? $TESTTMP/repo1/Renamed
  merging Renamed


  $ hg log -Gp -T '{desc}\n' --git Renamed A
  @  C
  ╷  diff --git a/Renamed b/Renamed
  ╷  --- a/Renamed
  ╷  +++ b/Renamed
  ╷  @@ -1,1 +1,1 @@
  ╷  -1
  ╷  +2
  ╷
  o  B
  │  diff --git a/A b/A
  │  deleted file mode 100644
  │  --- a/A
  │  +++ /dev/null
  │  @@ -1,1 +0,0 @@
  │  -1
  │  diff --git a/Renamed b/Renamed
  │  new file mode 100644
  │  --- /dev/null
  │  +++ b/Renamed
  │  @@ -0,0 +1,1 @@
  │  +1
  │
  o  A
     diff --git a/A b/A
     new file mode 100644
     --- /dev/null
     +++ b/A
     @@ -0,0 +1,1 @@
     +1

# status should not show "! A"

  $ hg status
