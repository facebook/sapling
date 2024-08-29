# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_files"
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > EOF

Setup helpers
  $ log() {
  >   hg log -G -T "{desc} [{phase};rev={rev};{node|short}] {remotenames}" "$@"
  > }

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
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

Pushrebase commit 1
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg push -r . --to master_bookmark
  pushing rev a0c9c5791058 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Remove all filenodes, make sure that update still works fine
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes where repo_id >= 0";
  $ hg up -q master_bookmark
