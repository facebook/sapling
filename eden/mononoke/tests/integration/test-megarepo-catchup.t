# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

  $ cd "$TESTTMP"  
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo a > a && hg add a && hg ci -m 'large repo first commit'
  $ echo b > b && hg add b && hg ci -m 'large repo second commit'
  $ hg book -r . pre_merge_head_bookmark
  $ hg book -r . head_bookmark

  $ hg up -q null
  $ mkdir smallrepofiles
  $ cd smallrepofiles
  $ mkdir unchanged_files
  $ cd unchanged_files
  $ for i in `seq 1 3`; do echo "$i" > "$i.out"; done
  $ cd ..
  $ mkdir to_change_files
  $ cd to_change_files
  $ for i in `seq 1 3`; do echo "$i" > "$i.out"; done
  $ cd ..
  $ mkdir to_move_files
  $ cd to_move_files
  $ for i in `seq 1 3`; do echo "$i" > "$i.out"; done
  $ cd ..
  $ hg addremove -q
  $ hg ci -m 'small repo first commit'
  $ hg book -r . small_repo_head_bookmark
  $ cd "$TESTTMP/repo-hg"

  $ hg up -q head_bookmark
  $ hg merge -q small_repo_head_bookmark
  $ hg ci -m 'invisible merge'

  $ echo "ab" > "ab"
  $ hg addremove -q
  $ hg commit -m "new commit in large repo"
  $ ls
  a
  ab
  b
  smallrepofiles
 

  $ hg up -q small_repo_head_bookmark
  $ cd smallrepofiles
  $ hg mv -q to_move_files moved_files
  $ hg ci -m "move files in small repo"
  $ cd to_change_files
  $ for i in `seq 1 3`; do echo "changed $i" > "$i.out"; done
  $ hg ci -m 'change files'
  $ cd ..
  $ ls
  moved_files
  to_change_files
  unchanged_files

  $ hg log -G
  @  commit:      f910c17f2a72
  |  bookmark:    small_repo_head_bookmark
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     change files
  |
  o  commit:      83c4b83dcc37
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     move files in small repo
  |
  | o  commit:      b662a919caea
  | |  bookmark:    head_bookmark
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     new commit in large repo
  | |
  | o  commit:      8eb1f2b968a3
  |/|  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     invisible merge
  | |
  o |  commit:      70b0bf7fe816
   /   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     small repo first commit
  |
  o  commit:      78a7e5a52cc8
  |  bookmark:    pre_merge_head_bookmark
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     large repo second commit
  |
  o  commit:      63d5c6ae8a3d
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     large repo first commit
  

  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-client --noupdate --config extensions.remotenames= -q

blobimport
  $ blobimport repo-hg/.hg repo

  $ megarepo_tool create-catchup-head-deletion-commits \
  > --head-bookmark head_bookmark \
  > --to-merge-cs-id small_repo_head_bookmark \
  > --path-regex "^smallrepofiles.*" \
  > --deletion-chunk-size 3 \
  > user "deletion commit" 
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * total files to delete is 6 (glob)
  * created bonsai #0. Deriving hg changeset for it to verify its correctness (glob)
  * derived *, pushrebasing... (glob)
  * Pushrebased to * (glob)
  * created bonsai #1. Deriving hg changeset for it to verify its correctness (glob)
  * derived *, pushrebasing... (glob)
  * Pushrebased to * (glob)
  $ mononoke
  $ wait_for_mononoke
  $ cd "$TESTTMP/repo-client"
  $ hgmn pull
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  adding remote bookmark head_bookmark
  adding remote bookmark pre_merge_head_bookmark
  adding remote bookmark small_repo_head_bookmark
  $ hgmn up head_bookmark
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark head_bookmark)
  $ ls
  a
  ab
  b
  smallrepofiles
  $ ls smallrepofiles
  unchanged_files
  $ hg log -G
  @  commit:      * (glob)
  |  bookmark:    head_bookmark
  |  user:        user
  |  date:        * (glob)
  |  summary:     [MEGAREPO CATCHUP DELETE] deletion commit (1)
  |
  o  commit:      * (glob)
  |  user:        user
  |  date:        * (glob)
  |  summary:     [MEGAREPO CATCHUP DELETE] deletion commit (0)
  |
  | o  commit:      f910c17f2a72
  | |  bookmark:    small_repo_head_bookmark
  | |  bookmark:    default/small_repo_head_bookmark
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     change files
  | |
  | o  commit:      83c4b83dcc37
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     move files in small repo
  | |
  o |  commit:      b662a919caea
  | |  bookmark:    default/head_bookmark
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     new commit in large repo
  | |
  o |  commit:      8eb1f2b968a3
  |\|  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     invisible merge
  | |
  | o  commit:      70b0bf7fe816
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     small repo first commit
  |
  o  commit:      * (glob)
  |  bookmark:    pre_merge_head_bookmark
  |  bookmark:    default/pre_merge_head_bookmark
  |  user:        test
  |  date:        * (glob)
  |  summary:     large repo second commit
  |
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     large repo first commit
  
