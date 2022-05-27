#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ CACHEDIR=`pwd`/hgcache

  $ setconfig experimental.allowfilepeer=True
  $ . "$TESTDIR/library.sh"

  $ hg init client1
  $ cd client1
  $ cat >> .hg/hgrc << 'EOF'
  > [remotefilelog]
  > reponame=master
  > cachepath=$CACHEDIR
  > EOF

  $ echo a > a
  $ mkdir dir
  $ echo b > dir/b
  $ hg commit -Aqm 'initial commit'

  $ hg init ../client2
  $ cd ../client2
  $ hg pull -q ../client1
