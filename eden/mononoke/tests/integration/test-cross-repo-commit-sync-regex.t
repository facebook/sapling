# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

setup configuration
Disable bookmarks cache because bookmarks are modified by two separate processes
  $ REPOTYPE="blob_files"
  $ NO_BOOKMARKS_CACHE=1 REPOID=0 REPONAME=meg-mon setup_common_config $REPOTYPE
  $ NO_BOOKMARKS_CACHE=1 REPOID=1 REPONAME=fbs-mon setup_common_config $REPOTYPE
  $ NO_BOOKMARKS_CACHE=1 REPOID=2 REPONAME=ovr-mon setup_common_config $REPOTYPE

  $ cat >> "$HGRCPATH" <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > pushrebase=
  > remotenames=
  > EOF

  $ setup_commitsyncmap
  $ setup_configerator_configs

-- setup hg server repos

  $ function createfile { mkdir -p "$(dirname  $1)" && echo "$1" > "$1" && hg add -q "$1"; }
  $ function createfile_with_content { mkdir -p "$(dirname  $1)" && echo "$2" > "$1" && hg add -q "$1"; }

-- init fbsource
  $ cd $TESTTMP
  $ hginit_treemanifest fbs-hg-srv
  $ cd fbs-hg-srv
-- create an initial commit, which will be the last_synced_commit
  $ createfile fbcode/fbcodefile_fbsource
  $ createfile arvr/arvrfile_fbsource
  $ createfile otherfile_fbsource
  $ hg -q ci -m "fbsource commit 1" && hg book -ir . master_bookmark

-- init ovrsource
  $ cd $TESTTMP
  $ hginit_treemanifest ovr-hg-srv
  $ cd ovr-hg-srv
  $ createfile fbcode/fbcodefile_ovrsource
  $ createfile arvr/arvrfile_ovrsource
  $ createfile otherfile_ovrsource
  $ createfile Research/researchfile_ovrsource
  $ hg -q ci -m "ovrsource commit 1" && hg book -r . master_bookmark

-- init megarepo - note that some paths are shifted, but content stays the same
  $ cd $TESTTMP
  $ hginit_treemanifest meg-hg-srv
  $ cd meg-hg-srv
  $ createfile fbcode/fbcodefile_fbsource
  $ createfile_with_content .fbsource-rest/arvr/arvrfile_fbsource arvr/arvrfile_fbsource
  $ createfile otherfile_fbsource
  $ createfile_with_content .ovrsource-rest/fbcode/fbcodefile_ovrsource fbcode/fbcodefile_ovrsource
  $ createfile arvr/arvrfile_ovrsource
  $ createfile_with_content arvr-legacy/otherfile_ovrsource otherfile_ovrsource
  $ createfile_with_content arvr-legacy/Research/researchfile_ovrsource Research/researchfile_ovrsource
  $ hg -q ci -m "megarepo commit 1"
  $ hg book -r . master_bookmark

-- blobimport hg servers repos into Mononoke repos
  $ cd "$TESTTMP"
  $ REPOID=0 blobimport meg-hg-srv/.hg meg-mon
  $ REPOID=1 blobimport fbs-hg-srv/.hg fbs-mon
  $ REPOID=2 blobimport ovr-hg-srv/.hg ovr-mon

-- get some bonsai hashes to avoid magic strings later
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 1 master_bookmark)
  $ OVRSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 2 master_bookmark)
  $ MEGAREPO_MERGE_BONSAI=$(get_bonsai_bookmark 0 master_bookmark)

-- insert sync mapping entry
  $ add_synced_commit_mapping_entry 1 $FBSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI
  $ add_synced_commit_mapping_entry 2 $OVRSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI

-- setup hg client repos
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/fbs-hg-srv fbs-hg-cnt --noupdate
  $ hgclone_treemanifest ssh://user@dummy/ovr-hg-srv ovr-hg-cnt --noupdate
  $ hgclone_treemanifest ssh://user@dummy/meg-hg-srv meg-hg-cnt --noupdate

-- start mononoke
  $ mononoke
  $ wait_for_mononoke

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_2', 2)";
  $ mononoke_x_repo_sync 2 0 tail --catch-up-once |& grep -E '(processing|skipping)'
  * processing log entry * (glob)

-- push to a bookmark that won't be synced
  $ cd "$TESTTMP"/ovr-hg-cnt
  $ REPONAME=ovr-mon hgmn up -q master_bookmark
  $ createfile arvr/somefile
  $ hg -q ci -m "ovrsource commit 2"
  $ REPONAME=ovr-mon hgmn push -r . --to somebookmark -q --create

-- now push to master
  $ REPONAME=ovr-mon hgmn up -q master_bookmark
  $ createfile arvr/newfile
  $ hg -q ci -m "ovrsource commit 3"
  $ REPONAME=ovr-mon hgmn push -r . --to master_bookmark -q

  $ mononoke_x_repo_sync 2 0 tail --bookmark-regex "master_bookmark" --catch-up-once |& grep -E '(processing|skipping)'
  * skipping log entry #4 for somebookmark (glob)
  * processing log entry * (glob)
