# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > sparse=
  > EOF

setup repo
  $ hginit_treemanifest repo
  $ cd repo
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

Clone the repo
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ cat >> .hgsparse_profile <<EOF
  > [include]
  > foo
  > EOF
  $ hg commit -Aqm 'Add sparse profile'
  $ hg sparse enable .hgsparse_profile
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
