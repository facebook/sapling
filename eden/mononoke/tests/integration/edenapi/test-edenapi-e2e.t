# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree. 

  $ . "${TEST_FIXTURES}/library.sh"
  $ configure modern
  $ setconfig remotenames.selectivepulldefault=master_bookmark \
  >  pull.httpcommitgraph2=1 pull.httphashprefix=1 pull.use-commit-graph=true clone.use-rust=true clone.use-commit-graph=true

Set up local hgrc and Mononoke config, with http pull
  $ setup_common_config
  $ cd $TESTTMP

Custom smartlog
  $ function smartlog {
  >  hg log -G -T "{node|short} {phase} '{desc|firstline}' {bookmarks} {remotenames}"
  > }

Initialize test repo.
  $ hginit_treemanifest repo
  $ cd repo
  $ mkcommit base_commit
  $ hg log -T '{short(node)}\n'
  8b2dca0c8a72
  $ hg bookmark master_bookmark -r 8b2dca0c8a72


Import and start mononoke
  $ cd $TESTTMP
  $ blobimport repo/.hg repo
  $ mononoke
  $ wait_for_mononoke

Clone 1 and 2
  $ hg clone mono:repo client1
  Cloning repo into $TESTTMP/client1
  Checking out 'master_bookmark'
  1 files updated
  $ hg clone mono:repo client2 -q
  $ cd client1
  $ smartlog
  @  8b2dca0c8a72 public 'base_commit'  remote/master_bookmark
  
  $ hg up remote/master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a > a && hg commit -m "new commit" -A a
  $ hg push --to master_bookmark
  pushing rev 8ca8131de573 to destination mono:repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark
  $ smartlog
  @  8ca8131de573 public 'new commit'  remote/master_bookmark
  │
  o  8b2dca0c8a72 public 'base_commit'
  

Clone 3
  $ cd $TESTTMP
This is a hack, it seems WBC may be stale, causing the test to be flaky. It needs a proper fix.
  $ sleep 3
  $ hg clone mono:repo client3
  Cloning repo into $TESTTMP/client3
  Checking out 'master_bookmark'
  2 files updated
  $ cd client3
  $ hg up remote/master_bookmark 
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ smartlog
  @  8ca8131de573 public 'new commit'  remote/master_bookmark
  │
  o  8b2dca0c8a72 public 'base_commit'
  
  $ echo b > a && hg commit -m "newer commit"
  $ hg push --to master_bookmark
  pushing rev 6b51b03e4f04 to destination mono:repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

Back to clone 1
  $ cd "$TESTTMP/client1"
This is a hack, it seems WBC may be stale, causing the test to be flaky. It needs a proper fix.
  $ sleep 3
  $ hg pull
  pulling from mono:repo
  searching for changes
  $ smartlog
  o  6b51b03e4f04 public 'newer commit'  remote/master_bookmark
  │
  @  8ca8131de573 public 'new commit'
  │
  o  8b2dca0c8a72 public 'base_commit'
  
  $ hg up remote/master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

On clone 2 with tailer
  $ cd "$TESTTMP/client2"
  $ hg pull
  pulling from mono:repo
  searching for changes
  $ smartlog
  o  6b51b03e4f04 public 'newer commit'  remote/master_bookmark
  │
  o  8ca8131de573 public 'new commit'
  │
  @  8b2dca0c8a72 public 'base_commit'
  
