# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree. 

  $ . "${TEST_FIXTURES}/library.sh"
  $ configure modern
  $ setconfig remotenames.selectivepulldefault=master_bookmark \
  >  pull.httpcommitgraph=1 pull.httphashprefix=1

Set up local hgrc and Mononoke config, with http pull
  $ setup_common_config
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > enabled=true
  > master_bookmark="master_bookmark"
  > CONFIG
  $ cd $TESTTMP

Custom smartlog
  $ function sl {
  >  hgedenapi log -G -T "{node|short} {phase} '{desc|firstline}' {bookmarks}"
  > }

Initialize test repo.
  $ hginit_treemanifest repo
  $ cd repo
  $ mkcommit base_commit
  $ hg log -T '{short(node)}\n'
  8b2dca0c8a72
  $ hgedenapi bookmark master_bookmark -r 8b2dca0c8a72


Import and start mononoke
  $ cd $TESTTMP
  $ blobimport repo/.hg repo
  $ quiet segmented_changelog_tailer_reseed --repo=repo --head=master_bookmark
  $ mononoke
  $ wait_for_mononoke

Clone 1 and 2
  $ hgedenapi clone "mononoke://$(mononoke_address)/repo" client1
  fetching lazy changelog
  populating main commit graph
  tip commit: 8b2dca0c8a726d66bf26d47835a356cc4286facd
  fetching selected remote bookmarks
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hgedenapi clone "mononoke://$(mononoke_address)/repo" client2 -q
  $ cd client1
  $ sl
  @  8b2dca0c8a72 public 'base_commit'
  
  $ hgedenapi up remote/master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a > a && hgedenapi commit -m "new commit" -A a
  $ hgedenapi push --to master_bookmark
  pushing rev 8ca8131de573 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark
  $ sl
  @  8ca8131de573 public 'new commit'
  │
  o  8b2dca0c8a72 public 'base_commit'
  

Clone 3
  $ cd $TESTTMP
This is a hack, it seems WBC may be stale, causing the test to be flaky. It needs a proper fix.
  $ sleep 3
  $ hgedenapi clone "mononoke://$(mononoke_address)/repo" client3
  fetching lazy changelog
  populating main commit graph
  tip commit: 8b2dca0c8a726d66bf26d47835a356cc4286facd
  fetching selected remote bookmarks
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd client3
  $ hgedenapi up remote/master_bookmark 
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl
  @  8ca8131de573 public 'new commit'
  │
  o  8b2dca0c8a72 public 'base_commit'
  
  $ echo b > a && hgedenapi commit -m "newer commit"
  $ hgedenapi push --to master_bookmark
  pushing rev 6b51b03e4f04 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

Back to clone 1
  $ cd "$TESTTMP/client1"
This is a hack, it seems WBC may be stale, causing the test to be flaky. It needs a proper fix.
  $ sleep 3
  $ hgedenapi pull
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  imported commit graph for 1 commit (1 segment)
  $ sl
  o  6b51b03e4f04 public 'newer commit'
  │
  @  8ca8131de573 public 'new commit'
  │
  o  8b2dca0c8a72 public 'base_commit'
  
  $ hgedenapi up remote/master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

On clone 2 with tailer
  $ cd "$TESTTMP/client2"
  $ quiet segmented_changelog_tailer_once --repo repo
  $ hgedenapi pull
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  imported commit graph for 2 commits (1 segment)
  $ sl
  o  6b51b03e4f04 public 'newer commit'
  │
  o  8ca8131de573 public 'new commit'
  │
  @  8b2dca0c8a72 public 'base_commit'
  
