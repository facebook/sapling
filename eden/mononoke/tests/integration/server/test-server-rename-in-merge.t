# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

setup repo
  $ hginit_treemanifest repo
  $ cd repo
  $ echo 1 > 1 && hg addremove && hg ci -m 1
  adding 1
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 2 > 2 && hg addremove && hg ci -m 2
  adding 2

Clone the repo
  $ cd ..
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ cd ../repo

Create merge commit with rename
  $ hg up -q "min(all())"
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg mv 1 2 --force
  $ hg ci -m merge
  $ hg st --change . -C
  A 2
    1
  R 1

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
  $ cd repo2
  $ hg pull
  pulling from mono:repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st --change . -C
  A 2
    1
  R 1
