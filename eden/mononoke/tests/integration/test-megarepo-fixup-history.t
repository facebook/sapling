# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repositories
  $ REPOTYPE="blob_files"
  $ FBS_REPOID=0

  $ NO_BOOKMARKS_CACHE=1 REPOID=$FBS_REPOID REPONAME=repo setup_common_config $REPOTYPE
  $ setup_commitsyncmap
  $ setup_configerator_configs
  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
  >     "force_unode_v2": true
  >   }
  > }
  > EOF

  $ cat >> "$HGRCPATH" <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

  $ function createfile { mkdir -p "$(dirname  $1)" && echo "$1" > "$1" && hg add -q "$1"; }

-- init hg fbsource server repo
  $ cd $TESTTMP
  $ hginit_treemanifest fbs-hg-srv
  $ cd fbs-hg-srv
-- create an initial commits
  $ createfile fbcode/file_with_correct_history
  $ hg -q ci -m "master commit 1"

  $ createfile fbcode/file_with_incorrect_history
  $ hg -q ci -m "commit commit 2 [incorrect history]"

  $ echo changed > fbcode/file_with_incorrect_history
  $ createfile file_with_incorrect_history2
  $ hg -q ci -m "commit commit 3 [incorrect history]"

  $ hg book -i -r . master

  $ hg update -q null

  $ createfile fbcode/file_with_incorrect_history

  $ hg -q ci -m "small repo commit 1"

  $ echo changed_ > fbcode/file_with_incorrect_history
  $ createfile file_with_incorrect_history2
  $ createfile fbcode/file_with_correct_history
  $ hg -q ci -m "small repo commit 2 [corrected history]"

  $ createfile some_file_that_should_stay_in_small_repo_only
  $ createfile some_file_that_should_stay_in_small_repo_only2
  $ createfile some_file_that_should_stay_in_small_repo_only3
  $ createfile some_file_that_should_stay_in_small_repo_only4
  $ hg -q ci -m "small repo commit 3"

  $ hg book -i -r . correct_history_branch

-- blobimport hg server repos into Mononoke repos
  $ cd "$TESTTMP"
  $ REPOID="$FBS_REPOID" blobimport "fbs-hg-srv/.hg" "repo"

-- setup hg client repos
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/fbs-hg-srv fbs-hg-cnt --noupdate

Start mononoke server
  $ start_and_wait_for_mononoke_server
  $ cat > "paths_to_fixup" <<EOF
  > fbcode/file_with_incorrect_history
  > file_with_incorrect_history2
  > EOF
  $ COMMIT_DATE="1985-09-04T00:00:00.00Z"
  $ REPOID=$FBS_REPOID megarepo_tool history-fixup-deletes author "history fixup" master correct_history_branch --paths-file paths_to_fixup --even-chunk-size 3 --commit-date-rfc3339 "$COMMIT_DATE" 2> /dev/null
  7d84767352730c2af3020ef0d16c1933438724b14a93a87462bcf24f02bc6fc1
  81ea05520fa72bf27124fed8d0e0be49683f4695e86c0b57940982291089a15d
  d6c0cb28cbef050857dcef87adfc509c6d01d7fec8a0423ebb41d1fa4f0158c9

  $ REPOID=$FBS_REPOID  megarepo_tool merge 7d84767352730c2af3020ef0d16c1933438724b14a93a87462bcf24f02bc6fc1 81ea05520fa72bf27124fed8d0e0be49683f4695e86c0b57940982291089a15d author "history fixup"  --mark-public --commit-date-rfc3339 "$COMMIT_DATE" --bookmark master 2> /dev/null

  $ cd "$TESTTMP"/fbs-hg-cnt
  $ REPONAME=repo hgmn pull -q

  $ hgmn update -q master

  $ ls *
  file_with_incorrect_history2
  
  fbcode:
  file_with_correct_history
  file_with_incorrect_history


  $ hg log -f fbcode/file_with_incorrect_history -T "{node} {desc}\n"
  6c017a8ba0a60b7a82b3cd0a98b52dc68def9f96 small repo commit 2 [corrected history]
  11fbaaa53e1b7d7fb87f3831b007c803fb64afa7 small repo commit 1

  $ hg log -f fbcode/file_with_correct_history -T "{node} {desc}\n"
  835251f7cda8fd1adddf414ce67d58090897e93a master commit 1

  $ hg debugchangelog --migrate fullsegments
  $ hg log -G -T "{desc} [{phase};{node|short}] {remotenames}" -r 'sort(::.,-topo,topo.firstbranch=desc("master commit"))' | sed '$d'
  @    history fixup [public;dcacf3dd28f1] default/master
  ├─╮
  │ o  [MEGAREPO DELETE] history fixup (1) [public;d3b2dfc1d7dc]
  │ │
  │ o  [MEGAREPO DELETE] history fixup (0) [public;c2a5523610c4]
  │ │
  │ o  small repo commit 3 [public;ea8595b036ed]
  │ │
  │ o  small repo commit 2 [corrected history] [public;6c017a8ba0a6]
  │ │
  │ o  small repo commit 1 [public;11fbaaa53e1b]
  │
  o  [MEGAREPO DELETE] history fixup (0) [public;94932f105be0]
  │
  o  commit commit 3 [incorrect history] [public;c3f812992511]
  │
  o  commit commit 2 [incorrect history] [public;4f27e05b6e2a]
  │
  o  master commit 1 [public;835251f7cda8]
