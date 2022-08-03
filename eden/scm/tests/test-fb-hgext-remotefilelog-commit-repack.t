#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ . $TESTDIR/library.sh

  $ setconfig experimental.allowfilepeer=True
  $ setconfig workingcopy.ruststatus=False
  $ hginit master
  $ cd master
  $ setconfig 'remotefilelog.server=True'
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master client
  streaming all changes
  0 files to transfer, 0 bytes of data
  transferred 0 bytes in 0.0 seconds (0 bytes/sec)
  no changes found
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd client

  $ setconfig 'remotefilelog.commitsperrepack=1'

  $ echo x > x
  $ hg commit -Am x
  (running background incremental repack)
  adding x
  (running background incremental repack)
  (running background incremental repack)
  (running background incremental repack)
