# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export EMIT_OBSMARKERS=1
  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

setup repo
  $ hginit_treemanifest repo
  $ cd repo
  $ drawdag <<EOF
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
  $ start_and_wait_for_mononoke_server
Clone the repo
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

Push commits that will be obsoleted
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ log -r "all()"
  @  2 [draft;rev=4;0c67ec8c24b9]
  │
  o  1 [draft;rev=3;a0c9c5791058]
  │
  │ o  C [public;rev=2;26805aba1e60] default/master_bookmark
  │ │
  │ o  B [public;rev=1;112478962961]
  ├─╯
  o  A [public;rev=0;426bada5c675]
  $
  $ hg push -r . --to master_bookmark
  pushing rev 0c67ec8c24b9 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  pushrebasing stack (426bada5c675, 0c67ec8c24b9] (2 commits) to remote bookmark master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to dc31470c8386
  $ log -r "all()"
  @  2 [public;rev=6;dc31470c8386] default/master_bookmark
  │
  o  1 [public;rev=5;c2e526aacb51]
  │
  o  C [public;rev=2;26805aba1e60]
  │
  o  B [public;rev=1;112478962961]
  │
  o  A [public;rev=0;426bada5c675]
  $

Push commits that will not be obsoleted
  $ hg up -q dc31470c8386
  $ echo 3 > 3 && hg add 3 && hg ci -m 3
  $ log -r "all()"
  @  3 [draft;rev=7;6398085ceb9d]
  │
  o  2 [public;rev=6;dc31470c8386] default/master_bookmark
  │
  o  1 [public;rev=5;c2e526aacb51]
  │
  o  C [public;rev=2;26805aba1e60]
  │
  o  B [public;rev=1;112478962961]
  │
  o  A [public;rev=0;426bada5c675]
  $
  $ hg push -r . --to master_bookmark
  pushing rev 6398085ceb9d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (dc31470c8386, 6398085ceb9d] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 6398085ceb9d
  $ log -r "all()"
  @  3 [public;rev=7;6398085ceb9d] default/master_bookmark
  │
  o  2 [public;rev=6;dc31470c8386]
  │
  o  1 [public;rev=5;c2e526aacb51]
  │
  o  C [public;rev=2;26805aba1e60]
  │
  o  B [public;rev=1;112478962961]
  │
  o  A [public;rev=0;426bada5c675]
  $
