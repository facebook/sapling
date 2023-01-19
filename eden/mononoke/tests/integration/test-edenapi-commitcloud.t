# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.


# Commit Cloud Test with as much Edenapi integration as possible
# the most important parts are:
#    pull.httpbookmarks    ENABLED
#    pull.httpcommitgraph/pull.httpcommitgraph2    ENABLED
#    pull.httphashprefix    ENABLED
#    pull.httpmutation      ENABLED
#    commitcloud.usehttpupload    ENABLED
#    exchange.httpcommitlookup    ENABLED


#testcases commitgraph commitgraph_v2

  $ . "${TEST_FIXTURES}/library.sh"
  $ configure modern
  $ setconfig ui.ignorerevnum=false


select version of commit graph
#if commitgraph_v2
  $ setconfig pull.httpcommitgraph2=true
#else
  $ setconfig pull.httpcommitgraph=true
#endif

setup custom smartlog
  $ function sl {
  >  hgedenapi log -G -T "{node|short} {phase} '{desc|firstline}' {bookmarks} {join(mutations % '(Rewritten using {operation} into {join(successors % \'{node|short}\', \', \')})', ' ')}"
  > }

setup configuration
  $ export READ_ONLY_REPO=1
  $ export LOG=pull
  $ INFINITEPUSH_ALLOW_WRITES=true \
  >   setup_common_config
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > enabled=true
  > heads_to_include = [
  >    { bookmark = "master" },
  > ]
  > CONFIG
  $ cd $TESTTMP

setup tunables
  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
  >     "mutation_advertise_for_infinitepush": true,
  >     "mutation_accept_for_infinitepush": true,
  >     "mutation_generate_for_draft": true
  >   },
  >   "killswitches_by_repo": {
  >     "repo": {
  >       "enable_writing_to_new_commit_graph": true
  >     }
  >   }
  > }
  > EOF

setup common configuration for these tests
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend =
  > commitcloud =
  > infinitepush =
  > rebase =
  > remotenames =
  > share =
  > [remotefilelog]
  > reponame=repo
  > [infinitepush]
  > server=False
  > httpbookmarks=true
  > [visibility]
  > enabled = true
  > [mutation]
  > record = true
  > enabled = true
  > date = 0 0
  > [commitcloud]
  > hostname = testhost
  > servicetype = local
  > servicelocation = $TESTTMP
  > owner_team = The Test Team
  > updateonmove = true
  > usehttpupload = true
  > [pull]
  > httphashprefix = true
  > httpbookmarks = true
  > [exchange]
  > httpcommitlookup = true
  > EOF

setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ mkcommit "base_commit"
  $ hg log -T '{short(node)}\n'
  8b2dca0c8a72
  $ hg bookmark master -r tip

Import and start mononoke
  $ cd $TESTTMP
  $ blobimport repo/.hg repo
  $ quiet segmented_changelog_tailer_reseed --repo=repo --head=master
  $ mononoke
  $ wait_for_mononoke

Clone 1 and 2
  $ hgedenapi clone "mononoke://$(mononoke_address)/repo" client1 -q
  DEBUG pull::httpbookmarks: edenapi fetched bookmarks: {'master': '8b2dca0c8a726d66bf26d47835a356cc4286facd'}
  DEBUG pull::fastpath: master: 8b2dca0c8a726d66bf26d47835a356cc4286facd (unchanged)
  $ hgedenapi clone "mononoke://$(mononoke_address)/repo" client2 -q
  DEBUG pull::httpbookmarks: edenapi fetched bookmarks: {'master': '8b2dca0c8a726d66bf26d47835a356cc4286facd'}
  DEBUG pull::fastpath: master: 8b2dca0c8a726d66bf26d47835a356cc4286facd (unchanged)

Connect client 1 and client 2 to Commit Cloud
  $ cd client1
  $ hgedenapi cloud join -q
  $ hgedenapi up master -q

  $ cd ..

  $ cd client2
  $ hgedenapi cloud join -q
  $ hgedenapi up master -q


Make commits in the first client, and sync it
  $ cd ../client1
  $ mkcommitedenapi "A"
  $ mkcommitedenapi "B"
  $ mkcommitedenapi "C"
  $ hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: head 'c4f3cf0b6f49' hasn't been uploaded yet
  edenapi: queue 3 commits for upload
  edenapi: queue 3 files for upload
  edenapi: uploaded 3 files
  edenapi: queue 3 trees for upload
  edenapi: uploaded 3 trees
  edenapi: uploading commit '929f2b9071cf032d9422b3cce9773cbe1c574822'...
  edenapi: uploading commit 'e3133a4a05d58526656505f837b4ec6a66fb2709'...
  edenapi: uploading commit 'c4f3cf0b6f491ac3a792a95a73a4f186836f08af'...
  edenapi: uploaded 3 changesets
  commitcloud: commits synchronized
  finished in * (glob)

  $ sl
  @  c4f3cf0b6f49 draft 'C'
  │
  o  e3133a4a05d5 draft 'B'
  │
  o  929f2b9071cf draft 'A'
  │
  o  8b2dca0c8a72 public 'base_commit'
  
Sync from the second client - the commits should appear
  $ cd ../client2
  $ hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: nothing to upload
  pulling c4f3cf0b6f49 from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  DEBUG pull::httpgraph: edenapi fetched graph node: * (glob)
  DEBUG pull::httpgraph: edenapi fetched graph node: * (glob)
  DEBUG pull::httpgraph: edenapi fetched graph node: * (glob)
  DEBUG pull::httpgraph: edenapi fetched graph with known 3 draft commits
  commitcloud: commits synchronized
  finished in * (glob)

  $ sl
  o  c4f3cf0b6f49 draft 'C'
  │
  o  e3133a4a05d5 draft 'B'
  │
  o  929f2b9071cf draft 'A'
  │
  @  8b2dca0c8a72 public 'base_commit'
  

Make commits from the second client and sync it
  $ mkcommitedenapi "D"
  $ mkcommitedenapi "E"
  $ mkcommitedenapi "F"
  $ hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: head 'c981069f3f05' hasn't been uploaded yet
  edenapi: queue 3 commits for upload
  edenapi: queue 3 files for upload
  edenapi: uploaded 3 files
  edenapi: queue 3 trees for upload
  edenapi: uploaded 3 trees
  edenapi: uploading commit '4594cad5305da610864d2fac2e1f289af29f2c80'...
  edenapi: uploading commit '5267c897028ead469dbc2ac682c64dd20e1e1453'...
  edenapi: uploading commit 'c981069f3f0504264ce2fc76e96d49d74d8b18ba'...
  edenapi: uploaded 3 changesets
  commitcloud: commits synchronized
  finished in * (glob)


On the first client, make a bookmark, then sync - the bookmark and the new commits should be synced
  $ cd ../client1
  $ hgedenapi bookmark -r "min(all())" new_bookmark
  $ hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: nothing to upload
  pulling c981069f3f05 from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  DEBUG pull::httpgraph: edenapi fetched graph node: * (glob)
  DEBUG pull::httpgraph: edenapi fetched graph node: * (glob)
  DEBUG pull::httpgraph: edenapi fetched graph node: * (glob)
  DEBUG pull::httpgraph: edenapi fetched graph with known 3 draft commits
  commitcloud: commits synchronized
  finished in * (glob)

  $ sl
  o  c981069f3f05 draft 'F'
  │
  o  5267c897028e draft 'E'
  │
  o  4594cad5305d draft 'D'
  │
  │ @  c4f3cf0b6f49 draft 'C'
  │ │
  │ o  e3133a4a05d5 draft 'B'
  │ │
  │ o  929f2b9071cf draft 'A'
  ├─╯
  o  8b2dca0c8a72 public 'base_commit' new_bookmark
  
 
On the first client rebase the stack
  $ hgedenapi rebase -s 4594cad5305d -d c4f3cf0b6f49
  rebasing 4594cad5305d "D"
  rebasing 5267c897028e "E"
  rebasing c981069f3f05 "F"
  $ hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: head 'f5aa28a22f7b' hasn't been uploaded yet
  edenapi: queue 3 commits for upload
  edenapi: queue 0 files for upload
  edenapi: queue 3 trees for upload
  edenapi: uploaded 3 trees
  edenapi: uploading commit 'd79a28423f1418c8d38f273372df978e45738f4a'...
  edenapi: uploading commit '8da26d088b8f7fb4b56dc4db5d0d356a643bfc25'...
  edenapi: uploading commit 'f5aa28a22f7bb48c14c733d0c731e6ca65882d4b'...
  edenapi: uploaded 3 changesets
  commitcloud: commits synchronized
  finished in * (glob)


On the second client sync it
  $ cd ../client2
  $ hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: nothing to upload
  pulling f5aa28a22f7b from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  DEBUG pull::httpgraph: edenapi fetched graph node: * (glob)
  DEBUG pull::httpgraph: edenapi fetched graph node: * (glob)
  DEBUG pull::httpgraph: edenapi fetched graph node: * (glob)
  DEBUG pull::httpgraph: edenapi fetched graph with known 3 draft commits
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: current revision c981069f3f05 has been moved remotely to f5aa28a22f7b
  updating to f5aa28a22f7b
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ sl
  @  f5aa28a22f7b draft 'F'
  │
  o  8da26d088b8f draft 'E'
  │
  o  d79a28423f14 draft 'D'
  │
  o  c4f3cf0b6f49 draft 'C'
  │
  o  e3133a4a05d5 draft 'B'
  │
  o  929f2b9071cf draft 'A'
  │
  o  8b2dca0c8a72 public 'base_commit' new_bookmark
  
Check mutation markers
  $ hgedenapi up c981069f3f05 -q
  $ sl
  o  f5aa28a22f7b draft 'F'
  │
  o  8da26d088b8f draft 'E'
  │
  o  d79a28423f14 draft 'D'
  │
  │ @  c981069f3f05 draft 'F'  (Rewritten using rebase into f5aa28a22f7b)
  │ │
  │ x  5267c897028e draft 'E'  (Rewritten using rebase into 8da26d088b8f)
  │ │
  │ x  4594cad5305d draft 'D'  (Rewritten using rebase into d79a28423f14)
  │ │
  o │  c4f3cf0b6f49 draft 'C'
  │ │
  o │  e3133a4a05d5 draft 'B'
  │ │
  o │  929f2b9071cf draft 'A'
  ├─╯
  o  8b2dca0c8a72 public 'base_commit' new_bookmark
  

On the second client hide all draft commits
  $ hgedenapi hide -r 'draft()'
  hiding commit 929f2b9071cf "A"
  hiding commit e3133a4a05d5 "B"
  hiding commit c4f3cf0b6f49 "C"
  hiding commit 4594cad5305d "D"
  hiding commit 5267c897028e "E"
  hiding commit c981069f3f05 "F"
  hiding commit d79a28423f14 "D"
  hiding commit 8da26d088b8f "E"
  hiding commit f5aa28a22f7b "F"
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  working directory now at 8b2dca0c8a72
  9 changesets hidden
  $ hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)
  $ hgedenapi up master -q

  $ sl
  @  8b2dca0c8a72 public 'base_commit' new_bookmark
  

On the first client check that all commits were hidden
  $ cd ../client1
  $ hgedenapi cloud sync
  commitcloud: synchronizing 'repo' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)
  $ hgedenapi up master -q

  $ sl
  @  8b2dca0c8a72 public 'base_commit' new_bookmark
  
