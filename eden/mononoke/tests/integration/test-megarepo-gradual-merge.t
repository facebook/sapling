# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

  $ cd $TESTTMP

setup hg server repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ drawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF
  $ drawdag <<EOF
  > F
  > |
  > E
  > |
  > D
  > EOF
  $ hg up -q tip
  $ ls 
  D
  E
  F
  $ hg rm F && hg ci -m 'rm F'
  $ hg rm E && hg ci -m 'rm E'
  $ hg book -r 0069ba24938a pre_deletion_commit
  $ hg book -r c5d76fe4f0c0 last_deletion_commit 
  $ hg book -r 26805aba1e60 head_bookmark
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  c5d76fe4f0c0 rm E
  │
  o  99cd22e92467 rm F
  │
  o  0069ba24938a F
  │
  o  cd488e83d208 E
  │
  o  058c1e1fb10a D
  
  o  26805aba1e60 C
  │
  o  112478962961 B
  │
  o  426bada5c675 A
  
  $ hg book
     head_bookmark             26805aba1e60
     last_deletion_commit      c5d76fe4f0c0
     pre_deletion_commit       0069ba24938a

  $ cd .. 
  $ blobimport repo-hg/.hg repo
  $ megarepo_tool gradual-merge \
  > stash \
  > "gradual merge" \
  > --pre-deletion-commit pre_deletion_commit \
  > --last-deletion-commit last_deletion_commit \
  > --bookmark head_bookmark \
  > --limit 1 2>&1 | grep 'merging'
  * merging 1 commits (glob)

  $ megarepo_tool gradual-merge \
  > stash \
  > "gradual merge" \
  > --pre-deletion-commit pre_deletion_commit \
  > --last-deletion-commit last_deletion_commit \
  > --bookmark head_bookmark 2>&1 | grep 'merging'
  * merging 2 commits (glob)

  $ mononoke
  $ wait_for_mononoke
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo --noupdate
  $ cd repo
  $ hgmn pull
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark head_bookmark
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  o    * [MEGAREPO GRADUAL MERGE] gradual merge (2) (glob)
  ├─╮
  │ o    * [MEGAREPO GRADUAL MERGE] gradual merge (1) (glob)
  │ ├─╮
  │ │ o    * [MEGAREPO GRADUAL MERGE] gradual merge (0) (glob)
  │ │ ├─╮
  │ │ │ o  c5d76fe4f0c0 rm E
  │ ├───╯
  │ o │  99cd22e92467 rm F
  ├─╯ │
  o   │  0069ba24938a F
  │   │
  o   │  cd488e83d208 E
  │   │
  o   │  058c1e1fb10a D
      │
      o  26805aba1e60 C
      │
      o  112478962961 B
      │
      o  426bada5c675 A
  
  $ hgmn up head_bookmark
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark head_bookmark)
  $ ls
  A
  B
  C
  D
  E
  F
