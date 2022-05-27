#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Test that journal and lfs wrap the share extension properly

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > journal=
  > lfs=
  > [lfs]
  > threshold=1000B
  > usercache=$TESTTMP/lfs-cache
  > EOF

  $ hg init repo
  $ cd repo
  $ echo s > smallfile
  $ hg commit -Aqm 'add small file'
  $ cd ..

  $ hg --config 'extensions.share=' share repo sharedrepo
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
