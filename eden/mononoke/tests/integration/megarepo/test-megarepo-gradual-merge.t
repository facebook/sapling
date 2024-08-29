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

  $ hginit_treemanifest repo
  $ cd repo
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
  $ blobimport repo/.hg repo
  $ megarepo_tool gradual-merge \
  > stash \
  > "gradual merge" \
  > --pre-deletion-commit pre_deletion_commit \
  > --last-deletion-commit last_deletion_commit \
  > --bookmark head_bookmark \
  > --limit 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(9b65f1881b4fac85aa2f82ea599274472d58938ad1520e9306aa98942b5b2db3)) (glob)
  * changeset resolved as: ChangesetId(Blake2(0f74833ca121d604c4a32d9df3826d1b5d5b8d6191c6f4cd5ed0b323b2d3c288)) (glob)
  * Finding all commits to merge... (glob)
  * 3 total commits to merge (glob)
  * Finding commits that haven't been merged yet... (glob)
  * changeset resolved as: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)) (glob)
  * merging 1 commits (glob)
  * Preparing to merge 9b65f1881b4fac85aa2f82ea599274472d58938ad1520e9306aa98942b5b2db3 (glob)
  * changeset resolved as: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)) (glob)
  * Created merge changeset * (glob)
  * Generated hg changeset * (glob)
  * Now running pushrebase... (glob)
  * Pushrebased to * (glob)

  $ megarepo_tool gradual-merge \
  > stash \
  > "gradual merge" \
  > --pre-deletion-commit pre_deletion_commit \
  > --last-deletion-commit last_deletion_commit \
  > --bookmark head_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(9b65f1881b4fac85aa2f82ea599274472d58938ad1520e9306aa98942b5b2db3)) (glob)
  * changeset resolved as: ChangesetId(Blake2(0f74833ca121d604c4a32d9df3826d1b5d5b8d6191c6f4cd5ed0b323b2d3c288)) (glob)
  * Finding all commits to merge... (glob)
  * 3 total commits to merge (glob)
  * Finding commits that haven't been merged yet... (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * merging 2 commits (glob)
  * Preparing to merge afd03e5c132921683e0e7023556448c4dddd6dcf8d639d0743c638c5410413d2 (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Created merge changeset * (glob)
  * Generated hg changeset * (glob)
  * Now running pushrebase... (glob)
  * Pushrebased to * (glob)
  * Preparing to merge 0f74833ca121d604c4a32d9df3826d1b5d5b8d6191c6f4cd5ed0b323b2d3c288 (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Created merge changeset * (glob)
  * Generated hg changeset * (glob)
  * Now running pushrebase... (glob)
  * Pushrebased to * (glob)

  $ cd "$TESTTMP"
  $ hg clone -q mono:repo client --noupdate
  $ cd client

  $ start_and_wait_for_mononoke_server

  $ hg pull
  pulling from mono:repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
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
  
  $ hg up head_bookmark
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls
  A
  B
  C
  D
  E
  F
