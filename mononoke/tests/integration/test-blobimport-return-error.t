# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup repo

  $ hg init repo-hg

Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > remotefilelog=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

  $ touch a
  $ hg add a
  $ hg ci -ma
  $ touch b
  $ hg add b
  $ hg ci -mb
  $ hg log
  changeset:   1:0e067c57feba
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
   (re)
  changeset:   0:3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)
  $ cd $TESTTMP

blobimport with missing first commit, it should fail
  $ setup_mononoke_config
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo --skip 1 --panic-fate=exit > /dev/null
  [1]
