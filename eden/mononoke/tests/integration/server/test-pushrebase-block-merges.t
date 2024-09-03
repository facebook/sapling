# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration
  $ export BLOCK_MERGES=1
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

Try to push merge commit
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg up -q "min(all())"
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ hg merge -q -r 3 && hg ci -m "merge 1 and 2"
  $ log -r ":"
  @    merge 1 and 2 [draft;rev=5;3e1c4ca1f9be]
  ├─╮
  │ o  2 [draft;rev=4;c9b2673d3218]
  │ │
  o │  1 [draft;rev=3;a0c9c5791058]
  ├─╯
  │ o  C [public;rev=2;26805aba1e60] default/master_bookmark
  │ │
  │ o  B [public;rev=1;112478962961]
  ├─╯
  o  A [public;rev=0;426bada5c675]
  $

  $ hg push -r . --to master_bookmark
  fallback reason: merge commit is not supported by EdenApi push yet
  pushing rev 3e1c4ca1f9be to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Pushrebase blocked because it contains a merge commit.
  remote:     If you need this for a specific use case please contact
  remote:     the Source Control team at https://fburl.com/27qnuyl2
  remote: 
  remote:   Root cause:
  remote:     Pushrebase blocked because it contains a merge commit.
  remote:     If you need this for a specific use case please contact
  remote:     the Source Control team at https://fburl.com/27qnuyl2
  remote: 
  remote:   Debug context:
  remote:     Error(
  remote:         "Pushrebase blocked because it contains a merge commit.\nIf you need this for a specific use case please contact\nthe Source Control team at https://fburl.com/27qnuyl2",
  remote:     )
  abort: unexpected EOL, expected netstring digit
  [255]
