# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ setconfig remotenames.selectivepulldefault=master_bookmark,head_bookmark,small_repo_head_bookmark,pre_merge_head_bookmark

  $ cd $TESTTMP
  $ testtool_drawdag --print-hg-hashes -R repo --derive-all --no-default-files <<EOF
  > A-B
  > C
  > # message: A "large repo first commit"
  > # modify: A a a 
  > # message: B "large repo second commit"
  > # modify: B b b 
  > # bookmark: B head_bookmark
  > # bookmark: B pre_merge_head_bookmark
  > # message: C "small repo first commit"
  > # modify: C smallrepofiles/unchanged_files/1.out 1 
  > # modify: C smallrepofiles/unchanged_files/2.out 2 
  > # modify: C smallrepofiles/unchanged_files/3.out 3 
  > # modify: C smallrepofiles/to_change_files/1.out 1 
  > # modify: C smallrepofiles/to_change_files/2.out 2 
  > # modify: C smallrepofiles/to_change_files/3.out 3 
  > # modify: C smallrepofiles/to_move_files/1.out 1 
  > # modify: C smallrepofiles/to_move_files/2.out 2 
  > # modify: C smallrepofiles/to_move_files/3.out 3 
  > # bookmark: C small_repo_head_bookmark
  > EOF
  A=fd091fd341e8ab8dbb75de80283dc27ca76a5244
  B=f90dd86918c852ca79f8c4945fd8bacc3207cffc
  C=7d49537551bb61168762073ac2169b6a8cde5448
 -- start mononoke
  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo --noupdate
  $ cd repo   


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
  $ hg push -q --to head_bookmark --create
 

  $ hg up -q small_repo_head_bookmark
  $ cd smallrepofiles
  $ hg mv -q to_move_files moved_files
  $ hg ci -m "move files in small repo"
  $ cd to_change_files
  $ for i in `seq 1 3`; do echo "changed $i" > "$i.out"; done
  $ hg ci -m 'change files'
  $ hg push -q --to small_repo_head_bookmark --create
  $ 
  $ cd ..
  $ ls
  moved_files
  to_change_files
  unchanged_files

  $ hg log -G
  @  commit:      0c9911fb7625
  │  bookmark:    remote/small_repo_head_bookmark
  │  hoistedname: small_repo_head_bookmark
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     change files
  │
  o  commit:      29d62cac93fb
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     move files in small repo
  │
  │ o  commit:      c7284e868e72
  │ │  bookmark:    remote/head_bookmark
  │ │  hoistedname: head_bookmark
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     new commit in large repo
  │ │
  │ o  commit:      341d51c0367b
  ╭─┤  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     invisible merge
  │ │
  o │  commit:      7d49537551bb
    │  user:        author
    │  date:        Thu Jan 01 00:00:00 1970 +0000
    │  summary:     small repo first commit
    │
    o  commit:      f90dd86918c8
    │  bookmark:    remote/pre_merge_head_bookmark
    │  hoistedname: pre_merge_head_bookmark
    │  user:        author
    │  date:        Thu Jan 01 00:00:00 1970 +0000
    │  summary:     large repo second commit
    │
    o  commit:      fd091fd341e8
       user:        author
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     large repo first commit
  

  $ cd "$TESTTMP"

  $ mononoke_admin megarepo create-catchup-head-deletion-commits \
  > --head-bookmark head_bookmark \
  > --bookmark small_repo_head_bookmark \
  > --path-regex "^smallrepofiles.*" \
  > --deletion-chunk-size 3 \
  > --commit-author "user" \
  > --repo-name "repo" \
  > --commit-message "[MEGAREPO CATCHUP DELETE] deletion commit"
  * total files to delete is 6 (glob)
  * created bonsai #0. Deriving hg changeset for it to verify its correctness (glob)
  * derived *, pushrebasing... (glob)
  * Pushrebased to * (glob)
  * created bonsai #1. Deriving hg changeset for it to verify its correctness (glob)
  * derived *, pushrebasing... (glob)
  * Pushrebased to * (glob)

  $ cd "$TESTTMP/repo"
  $ hg pull
  pulling from mono:repo
  searching for changes
  $ hg up head_bookmark
  3 files updated, 0 files merged, 6 files removed, 0 files unresolved
  $ ls
  a
  ab
  b
  smallrepofiles
  $ ls smallrepofiles
  unchanged_files
  $ hg log -G
  @  commit:      * (glob)
  │  bookmark:    remote/head_bookmark
  │  hoistedname: head_bookmark
  │  user:        user
  │  date:        * (glob)
  │  summary:     [MEGAREPO CATCHUP DELETE] deletion commit
  │
  o  commit:      * (glob)
  │  user:        user
  │  date:        * (glob)
  │  summary:     [MEGAREPO CATCHUP DELETE] deletion commit
  │
  │ o  commit:      0c9911fb7625
  │ │  bookmark:    remote/small_repo_head_bookmark
  │ │  hoistedname: small_repo_head_bookmark
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     change files
  │ │
  │ o  commit:      29d62cac93fb
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     move files in small repo
  │ │
  o │  commit:      c7284e868e72
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     new commit in large repo
  │ │
  o │  commit:      341d51c0367b
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     invisible merge
  │ │
  │ o  commit:      7d49537551bb
  │    user:        author
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     small repo first commit
  │
  o  commit:      * (glob)
  │  bookmark:    remote/pre_merge_head_bookmark
  │  hoistedname: pre_merge_head_bookmark
  │  user:        author
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     large repo second commit
  │
  o  commit:      * (glob)
     user:        author
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     large repo first commit
  
