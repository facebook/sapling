# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repositories
  $ REPOTYPE="blob_files"
  $ FBS_REPOID=0

  $ REPOID=$FBS_REPOID REPONAME=repo setup_common_config $REPOTYPE
  $ setconfig remotenames.selectivepulldefault=master_bookmark,correct_history_branch
  $ setup_commitsyncmap
  $ setup_configerator_configs

  $ cat >> "$HGRCPATH" <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

  $ function createfile { mkdir -p "$(dirname  $1)" && echo "$1" > "$1" && hg add -q "$1"; }
  $ cd $TESTTMP
  $ testtool_drawdag --print-hg-hashes -R repo --derive-all --no-default-files <<EOF
  > A-B-C
  > D-E-F
  > # message: A "master commit 1"
  > # modify: A fbcode/file_with_correct_history fbcode/file_with_correct_history 
  > # message: B "commit commit 2 [incorrect history]"
  > # modify: B fbcode/file_with_incorrect_history fbcode/file_with_incorrect_history 
  > # message: C "commit commit 3 [incorrect history]"
  > # modify: C fbcode/file_with_incorrect_history changed 
  > # modify: E file_with_incorrect_history2 file_with_incorrect_history2 
  > # bookmark: C master_bookmark
  > # message: D "small repo commit 1"
  > # modify: D fbcode/file_with_incorrect_history fbcode/file_with_incorrect_history 
  > # message: E "small repo commit 2 [corrected history]"
  > # modify: E fbcode/file_with_incorrect_history changed_ 
  > # modify: E file_with_incorrect_history2 file_with_incorrect_history2 
  > # modify: E fbcode/file_with_correct_history fbcode/file_with_correct_history 
  > # message: F "small repo commit 3"
  > # modify: F some_file_that_should_stay_in_small_repo_only some_file_that_should_stay_in_small_repo_only 
  > # modify: F some_file_that_should_stay_in_small_repo_only2 some_file_that_should_stay_in_small_repo_only2 
  > # modify: F some_file_that_should_stay_in_small_repo_only3 some_file_that_should_stay_in_small_repo_only3 
  > # modify: F some_file_that_should_stay_in_small_repo_only4 some_file_that_should_stay_in_small_repo_only4 
  > # bookmark: F correct_history_branch
  > EOF
  A=73d782e237be52aed2e83a6636ea6bf23e2a722d
  B=31fc1dd174880c9a063e479a5c948a2b97413439
  C=86a443ecd51756e95e0a7fb050a7b944d1af000c
  D=40421eb6c5db40b99b8734c5a51164d62cf89e9c
  E=a7342374cb961b11d42371db14e86e2ff85aae31
  F=3bde3272bdeebb1002cba95eae55c3213d9199a3

 -- start mononoke
  $ cd $TESTTMP
  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo fbs-hg-cnt --noupdate
  $ cd fbs-hg-cnt 


-- blobimport hg server repos into Mononoke repos
  $ cd "$TESTTMP"

-- setup hg client repos
  $ cd "$TESTTMP"

Start mononoke server
  $ start_and_wait_for_mononoke_server
  $ cat > "paths_to_fixup" <<EOF
  > fbcode/file_with_incorrect_history
  > file_with_incorrect_history2
  > EOF
  $ COMMIT_DATE="1985-09-04T00:00:00.00Z"
  $ mononoke_admin megarepo history-fixup-deletes --repo-id 0 -a author -m "history fixup" --fixup-commit -B master_bookmark --correct-history-commit -B correct_history_branch --paths-file paths_to_fixup --even-chunk-size 3 --commit-date-rfc3339 "$COMMIT_DATE" 2> /dev/null
  0f198a7eb504dc4b6727e20923c890975c6b7d80d0f0f77ccf1125f71c66968c
  e2da10db549f3627629d8fdf26374f2925d0de1c13171bdaae16d203a9f22107
  30b2988958c818712867dd42f109b1a2a74ae3797755b0ce0116f814b9c719a9

  $ REPOID=$FBS_REPOID mononoke_admin megarepo merge --repo-id 0 \
  > -i 0f198a7eb504dc4b6727e20923c890975c6b7d80d0f0f77ccf1125f71c66968c \
  > -i e2da10db549f3627629d8fdf26374f2925d0de1c13171bdaae16d203a9f22107 \
  > -a author -m "history fixup" --mark-public --commit-date-rfc3339 "$COMMIT_DATE" \
  > --set-bookmark master_bookmark 2> /dev/null

  $ cd "$TESTTMP"/fbs-hg-cnt
  $ hg pull -q

  $ hg update -q master_bookmark

  $ ls *
  file_with_incorrect_history2
  
  fbcode:
  file_with_correct_history
  file_with_incorrect_history


  $ hg log -f fbcode/file_with_incorrect_history -T "{node} {desc}\n"
  a7342374cb961b11d42371db14e86e2ff85aae31 small repo commit 2 [corrected history]
  40421eb6c5db40b99b8734c5a51164d62cf89e9c small repo commit 1

  $ hg log -f fbcode/file_with_correct_history -T "{node} {desc}\n"
  a7342374cb961b11d42371db14e86e2ff85aae31 small repo commit 2 [corrected history]

  $ hg log -G -T "{desc} [{phase};{node|short}] {remotenames}" -r 'sort(::.,topo)' | sed '$d'
  @    history fixup [public;10161e9b9546] remote/master_bookmark
  ├─╮
  │ o  [MEGAREPO DELETE] history fixup (0) [public;81b3048989ea]
  │ │
  │ o  commit commit 3 [incorrect history] [public;86a443ecd517]
  │ │
  │ o  commit commit 2 [incorrect history] [public;31fc1dd17488]
  │ │
  │ o  master commit 1 [public;73d782e237be]
  │
  o  [MEGAREPO DELETE] history fixup (1) [public;ed9b99e55b9b]
  │
  o  [MEGAREPO DELETE] history fixup (0) [public;c9975714efaf]
  │
  o  small repo commit 3 [public;3bde3272bdee] remote/correct_history_branch
  │
  o  small repo commit 2 [corrected history] [public;a7342374cb96]
  │
  o  small repo commit 1 [public;40421eb6c5db]
