# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ setconfig remotenames.selectivepulldefault=master_bookmark,head_bookmark

  $ cd $TESTTMP

setup hg server repo

  $ testtool_drawdag --print-hg-hashes -R repo --derive-all <<EOF
  > A-B-C
  > D-E-F
  > # bookmark: C head_bookmark
  > # bookmark: F pre_deletion_commit
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8
  D=201c657038eaf07b35da8038dbb804e288510bc5
  E=568617fd5717c431962b3ebe8732567a5f6bc0f6
  F=d43882032b87ce7b8392e90858b2715c258c1dbd

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo --noupdate
  $ cd repo 
  $ hg up -q $F
  $ ls 
  D
  E
  F

  $ hg rm F && hg ci -m 'rm F'
  $ hg rm E && hg ci -m 'rm E'
  $ hg push -q -r . --to last_deletion_commit --create
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  9fd6e30353f9 rm E
  │
  o  5ce0eb3262a7 rm F
  │
  o  d43882032b87 F
  │
  o  568617fd5717 E
  │
  o  201c657038ea D
  
  o  d3b399ca8757 C
  │
  o  80521a640a0c B
  │
  o  20ca2a4749a4 A
  
  $ hg book
  no bookmarks set

  $ cd .. 
  $ mononoke_admin megarepo gradual-merge \
  > --repo-id 0 -a stash \
  > -m "gradual merge" \
  > --pre-deletion-commit -B pre_deletion_commit \
  > --last-deletion-commit -B last_deletion_commit \
  > --target-bookmark head_bookmark \
  > --limit 1
  * Finding all commits to merge... (glob)
  * 3 total commits to merge (glob)
  * Finding commits that haven't been merged yet... (glob)
  * merging 1 commits (glob)
  * Preparing to merge 8580abe13a55f056417a8e3fd2dd2744863634f3e4829c574df8c150d427ea82 (glob)
  * Created merge changeset * (glob)
  * Generated hg changeset * (glob)
  * Now running pushrebase... (glob)
  * Pushrebased to * (glob)

  $ mononoke_admin megarepo gradual-merge \
  > --repo-id 0 -a stash \
  > -m "gradual merge" \
  > --pre-deletion-commit -B pre_deletion_commit \
  > --last-deletion-commit -B last_deletion_commit \
  > --target-bookmark head_bookmark
  * Finding all commits to merge... (glob)
  * 3 total commits to merge (glob)
  * Finding commits that haven't been merged yet... (glob)
  * merging 2 commits (glob)
  * Preparing to merge 167cdda0845f8591a9ac5819d6383a56037a49c8569ffada217efd5d46ac4834 (glob)
  * Created merge changeset * (glob)
  * Generated hg changeset * (glob)
  * Now running pushrebase... (glob)
  * Pushrebased to * (glob)
  * Preparing to merge b2512cd96574be418a528a83a7e1e28c73e34fa4eac359083c381d2ae054cc39 (glob)
  * Created merge changeset * (glob)
  * Generated hg changeset * (glob)
  * Now running pushrebase... (glob)
  * Pushrebased to * (glob)

  $ cd "$TESTTMP"
  $ cd repo

  $ start_and_wait_for_mononoke_server

  $ hg pull
  pulling from mono:repo
  searching for changes
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  o    * [MEGAREPO GRADUAL MERGE] gradual merge (2) (glob)
  ├─╮
  │ o    * [MEGAREPO GRADUAL MERGE] gradual merge (1) (glob)
  │ ├─╮
  │ │ o    * [MEGAREPO GRADUAL MERGE] gradual merge (0) (glob)
  │ │ ├─╮
  │ │ │ @  9fd6e30353f9 rm E
  │ ├───╯
  │ o │  5ce0eb3262a7 rm F
  ├─╯ │
  o   │  d43882032b87 F
  │   │
  o   │  568617fd5717 E
  │   │
  o   │  201c657038ea D
      │
      o  d3b399ca8757 C
      │
      o  80521a640a0c B
      │
      o  20ca2a4749a4 A
  
  $ hg up head_bookmark
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls
  A
  B
  C
  D
  E
  F
