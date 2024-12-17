# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export EMIT_OBSMARKERS=1
  $ setconfig push.edenapi=true
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

setup repo
  $ quiet testtool_drawdag -R repo <<EOF
  > C
  > |
  > B
  > |
  > A
  > # bookmark: C master_bookmark
  > EOF

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
  @  2 [draft;rev=281474976710657;8b01ec816b8a]
  │
  o  1 [draft;rev=281474976710656;26f143b427a3]
  │
  │ o  C [public;rev=2;d3b399ca8757] remote/master_bookmark
  │ │
  │ o  B [public;rev=1;80521a640a0c]
  ├─╯
  o  A [public;rev=0;20ca2a4749a4]
  $
  $ hg push -r . --to master_bookmark
  pushing rev 8b01ec816b8a to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  pushrebasing stack (20ca2a4749a4, 8b01ec816b8a] (2 commits) to remote bookmark master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to b901ae25ceae
  $ log -r "all()"
  @  2 [public;rev=4;b901ae25ceae] remote/master_bookmark
  │
  o  1 [public;rev=3;c39a1f67cdbc]
  │
  o  C [public;rev=2;d3b399ca8757]
  │
  o  B [public;rev=1;80521a640a0c]
  │
  o  A [public;rev=0;20ca2a4749a4]
  $

Push commits that will not be obsoleted
  $ hg up -q b901ae25ceae
  $ echo 3 > 3 && hg add 3 && hg ci -m 3
  $ log -r "all()"
  @  3 [draft;rev=281474976710658;fff137c78c14]
  │
  o  2 [public;rev=4;b901ae25ceae] remote/master_bookmark
  │
  o  1 [public;rev=3;c39a1f67cdbc]
  │
  o  C [public;rev=2;d3b399ca8757]
  │
  o  B [public;rev=1;80521a640a0c]
  │
  o  A [public;rev=0;20ca2a4749a4]
  $
  $ hg push -r . --to master_bookmark
  pushing rev fff137c78c14 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (b901ae25ceae, fff137c78c14] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to fff137c78c14
  $ log -r "all()"
  @  3 [public;rev=5;fff137c78c14] remote/master_bookmark
  │
  o  2 [public;rev=4;b901ae25ceae]
  │
  o  1 [public;rev=3;c39a1f67cdbc]
  │
  o  C [public;rev=2;d3b399ca8757]
  │
  o  B [public;rev=1;80521a640a0c]
  │
  o  A [public;rev=0;20ca2a4749a4]
  $
