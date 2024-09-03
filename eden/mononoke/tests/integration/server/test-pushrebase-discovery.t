# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

This is the test to cover tricky case in the discovery logic.
Previously Mononoke's known() wireproto method returned `true` for both public and
draft commits. The problem was in that it affects pushrebase. If Mononoke
returns true for a draft commit and client runs `hg push -r HASH --to BOOK`,
then hg client logic may decide to just move a bookmark instead of running the
actual pushrebase.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 INFINITEPUSH_ALLOW_WRITES=true setup_common_config
  $ cd "$TESTTMP/mononoke-config"

  $ cat >> repos/repo/server.toml <<CONFIG
  > [[bookmarks]]
  > name="master_bookmark"
  > CONFIG

  $ register_hook always_fail_changeset <(
  >   echo 'bypass_pushvar="BYPASS_REVIEW=true"'
  > )


setup common configuration
  $ cd $TESTTMP
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > infinitepush=
  > commitcloud=
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
  $ hg clone -q mono:repo repo3 --noupdate
  $ cd repo2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg addremove -q
  $ hg ci -m 'to push'

Unsuccessful push creates a draft commit on the server
  $ hg push -r . --to master_bookmark
  pushing rev 812eca0823f9 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (426bada5c675, 812eca0823f9] (1 commit) to remote bookmark master_bookmark
  abort: Server error: hooks failed:
    always_fail_changeset for 15813a35298ec5ddc75d8d1c963f8d53435e00867cef3db930a54d173fec366f: This hook always fails
  [255]

In order to hit an edge case the master on the server needs to point to another commit.
Let's make a push
  $ cd ../repo3
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF
  $ hg up -q "min(all())"
  $ echo 2 > 2 && hg addremove -q
  $ hg ci -m 'to push2'
  $ hg push -r . --to master_bookmark --pushvar BYPASS_REVIEW=true -q

Now let's push the same commit again but with a bypass. It should pushrebase,
not move a bookmark
  $ cd ../repo2
  $ hg push -r . --to master_bookmark --pushvar BYPASS_REVIEW=true -q --config push.skip-cleanup-commits=true
  $ hg up -q master_bookmark
  $ log
  @  to push [public;rev=5;a6205c464622] default/master_bookmark
  │
  o  to push2 [public;rev=4;854b7c3bdd1f]
  │
  │ o  to push [draft;rev=3;812eca0823f9]
  │ │
  o │  C [public;rev=2;26805aba1e60]
  │ │
  o │  B [public;rev=1;112478962961]
  ├─╯
  o  A [public;rev=0;426bada5c675]
  $

The same procedure, but with commit cloud commit
  $ hg up -q "min(all())"
  $ echo commitcloud > commitcloud && hg addremove -q
  $ hg ci -m commitcloud
  $ hg cloud backup -q

Move master again
  $ cd ../repo3
  $ hg up -q "min(all())"
  $ echo 3 > 3 && hg addremove -q
  $ hg ci -m 'to push3'
  $ hg push -r . --to master_bookmark --pushvar BYPASS_REVIEW=true -q

Now let's push commit cloud commit. Again, it should do pushrebase
  $ cd ../repo2
  $ hg push -r . --to master_bookmark --pushvar BYPASS_REVIEW=true -q --config push.skip-cleanup-commits=true
  $ hg up -q master_bookmark
  $ log
  @  commitcloud [public;rev=8;3308f3bd8048] default/master_bookmark
  │
  o  to push3 [public;rev=7;c3f020572849]
  │
  │ o  commitcloud [draft;rev=6;17f29bea0858]
  │ │
  o │  to push [public;rev=5;a6205c464622]
  │ │
  o │  to push2 [public;rev=4;854b7c3bdd1f]
  │ │
  │ │ o  to push [draft;rev=3;812eca0823f9]
  │ ├─╯
  o │  C [public;rev=2;26805aba1e60]
  │ │
  o │  B [public;rev=1;112478962961]
  ├─╯
  o  A [public;rev=0;426bada5c675]
  $
