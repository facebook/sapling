  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
Disable bookmarks cache because bookmarks are modified by two separate processes
  $ REPOTYPE="blob:files"
  $ NO_BOOKMARKS_CACHE=1 REPOID=0 REPONAME=meg-mon setup_common_config $REPOTYPE
  $ NO_BOOKMARKS_CACHE=1 REPOID=1 REPONAME=fbs-mon setup_common_config $REPOTYPE
  $ NO_BOOKMARKS_CACHE=1 REPOID=2 REPONAME=ovr-mon setup_common_config $REPOTYPE
  $ setup_commitsyncmap

setup hg server repos
  $ function createfile { mkdir -p "$(dirname  $1)" && echo "$1" > "$1" && hg add -q "$1"; }

  $ cd $TESTTMP
  $ hginit_treemanifest fbs-hg-srv
  $ cd fbs-hg-srv
-- create an initial commit, which will be the last_synced_commit
  $ createfile fbcode/fbcodefile_fbsource
  $ createfile arvr/arvrfile_fbsource
  $ createfile otherfile_fbsource
  $ hg -q ci -m "fbsource commit 1" && hg book -ir . fbsource_c1
-- create an older version of fbsource_master, with a single simple change
  $ createfile fbcode/fbcodefile2_fbsource
  $ createfile arvr/arvrfile2_fbsource
  $ hg -q ci -m "fbsource commit 2" && hg book -ir . fbsource_master
-- create newer version fbsource_master_newer with more complex changes and more commits
  $ createfile fbcode/fbcodefile3_fbsource
  $ hg -q ci -m "fbsource commit 3"
  $ hg -q cp fbcode/fbcodefile3_fbsource fbcode/fbcodefile3_copy_fbsource
  $ hg -q ci -m "fbsource commit 4 (with copy of preserved path into preserved path)"
  $ hg -q cp arvr/arvrfile_fbsource arvr/arvrfile_copy_fbsource
  $ hg -q ci -m "fbsource commit 5 (with copy of moved path into moved path)"
  $ hg -q cp arvr/arvrfile_fbsource fbcode/arvrfile_copy_fbsource
  $ hg -q ci -m "fbsource commit 6 (with copy of moved path into preserved path)"
  $ hg -q cp fbcode/fbcodefile_fbsource arvr/fbcodefile_fbsource
  $ hg -q ci -m "fbsource commit 7 (with copy of preserved path into moved path)"
  $ hg -q rm arvr/fbcodefile_fbsource
  $ hg -q ci -m "fbsource commit 8 (with removal of a moved path)"
  $ hg -q rm fbcode/arvrfile_copy_fbsource
  $ hg -q ci -m "fbsource commit 9 (with removal of a preserved path)"
  $ hg book -ir . fbsource_master_newer
-- create newest version of fbsource_master, to test autodetection of sync start
  $ createfile fbcode/fbcodefile4_fbsource
  $ hg -q ci -m "fbsource commit 10"
  $ hg book -ir . fbsource_master_newest

-- let us create a non-master branch of commits
  $ hg -q up --inactive fbsource_c1
  $ createfile fbcode/fbcodebranch_fbsource
  $ hg -q ci -m "fbsource branch" && hg book -ir . fbsource_branch

  $ cd $TESTTMP
  $ hginit_treemanifest ovr-hg-srv
  $ cd ovr-hg-srv
  $ createfile fbcode/fbcodefile_ovrsource
  $ createfile arvr/arvrfile_ovrsource
  $ createfile otherfile_ovrsource
  $ createfile Research/researchfile_ovrsource
  $ hg -q ci -m "ovrsource commit 1" && hg book -r . ovrsource_c1
  $ createfile arvr/arvrfile2_ovrsource
  $ createfile fbcode/fbcodefile2_ovrsource
  $ createfile Research/researchfile2_ovrsource
  $ hg -q ci -m "ovrsource commit 2" && hg book -r . ovrsource_master

  $ cd $TESTTMP
  $ hginit_treemanifest meg-hg-srv
  $ cd meg-hg-srv
  $ createfile fbcode/fbcodefile_fbsource
  $ createfile .fbsource-rest/arvr/arvrfile_fbsource
  $ createfile otherfile_fbsource
  $ createfile .ovrsource-rest/fbcode/fbcodefile_ovrsource
  $ createfile arvr/arvrfile_ovrsource
  $ createfile arvr-legacy/otherfile_ovrsource
  $ createfile arvr-legacy/Research/researchfile_ovrsource
  $ hg -q ci -m "megarepo commit 1"
  $ hg book -r . megarepo_master

blobimport hg servers repos into Mononoke repos
  $ cd $TESTTMP
  $ REPOID=0 blobimport meg-hg-srv/.hg meg-mon
  $ REPOID=1 blobimport fbs-hg-srv/.hg fbs-mon
  $ REPOID=2 blobimport ovr-hg-srv/.hg ovr-mon

get some bonsai hashes to avoid magic strings later
  $ function get_bonsai_bookmark() {
  >   local bookmark repoid_backup
  >   repoid_backup=$REPOID
  >   export REPOID="$1"
  >   bookmark="$2"
  >   mononoke_admin bookmarks get -c bonsai "$bookmark" 2>/dev/null | cut -d' ' -f2
  >   export REPOID=$repoid_backup
  > }

  $ FBSOURCE_C1_BONSAI=$(get_bonsai_bookmark 1 fbsource_c1)
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 1 fbsource_master)
  $ OVRSOURCE_C1_BONSAI=$(get_bonsai_bookmark 2 ovrsource_c1)
  $ OVRSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 2 ovrsource_master)
  $ MEGAREPO_MERGE_BONSAI=$(get_bonsai_bookmark 0 megarepo_master)

setup hg client repos
  $ hgclone_treemanifest ssh://user@dummy/fbs-hg-srv fbs-hg-cnt --noupdate
  $ hgclone_treemanifest ssh://user@dummy/ovr-hg-srv ovr-hg-cnt --noupdate
  $ hgclone_treemanifest ssh://user@dummy/meg-hg-srv meg-hg-cnt --noupdate

start mononoke server
  $ mononoke
  $ wait_for_mononoke "$TESTTMP/repo"

run the sync, expected to fail, as parent of the synced commit is not present in the mapping
  $ mononoke_x_repo_sync_once 1 0 megarepo_master once --commit fbsource_master
  * using repo "fbs-mon" repoid RepositoryId(1) (glob)
  * using repo "meg-mon" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Checking if * is already synced 1->0 (glob)
  * Done preparing * (glob)
  * Parent commit 3478f726ba230a5071ed5fc3ff32fb99738365cdf1a335830576e3c2664706c1 hasn't been remapped (glob)
  * Queue size is 0 (glob)
  * Parent commit 3478f726ba230a5071ed5fc3ff32fb99738365cdf1a335830576e3c2664706c1 hasn't been remapped (glob)
  [1]

insert sync mapping entry
  $ add_synced_commit_mapping_entry 1 $FBSOURCE_C1_BONSAI 0 $MEGAREPO_MERGE_BONSAI
  $ add_synced_commit_mapping_entry 2 $OVRSOURCE_C1_BONSAI 0 $MEGAREPO_MERGE_BONSAI

let us make sure that we cannot sync the changeset if it is already synced
  $ mononoke_x_repo_sync_once 1 0 megarepo_master once --commit fbsource_c1
  * using repo "fbs-mon" repoid RepositoryId(1) (glob)
  * using repo "meg-mon" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Checking if * is already synced 1->0 (glob)
  * is already synced (glob)

run the sync again, from fbsource first
  $ mononoke_x_repo_sync_once 1 0 megarepo_master once --commit fbsource_master
  * using repo "fbs-mon" repoid RepositoryId(1) (glob)
  * using repo "meg-mon" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Checking if * is already synced 1->0 (glob)
  * Done preparing * (glob)
  * synced as * in *ms (glob)
  * Queue size is 0 (glob)

check that the changes are synced
  $ cd $TESTTMP/meg-hg-cnt
  $ REPONAME=meg-mon hgmn -q pull
  $ REPONAME=meg-mon hgmn -q status --change megarepo_master 2>/dev/null
  A .fbsource-rest/arvr/arvrfile2_fbsource
  A fbcode/fbcodefile2_fbsource

run the sync from ovrsource now
  $ mononoke_x_repo_sync_once 2 0 megarepo_master once --commit ovrsource_master
  * using repo "ovr-mon" repoid RepositoryId(2) (glob)
  * using repo "meg-mon" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Checking if * is already synced 2->0 (glob)
  * Done preparing * (glob)
  * synced as * in *ms (glob)
  * Queue size is 0 (glob)

this is after we also synced the ovrsource commit
  $ cd $TESTTMP/meg-hg-cnt
  $ REPONAME=meg-mon hgmn -q pull
  $ REPONAME=meg-mon hgmn -q status --change megarepo_master 2>/dev/null
  A .ovrsource-rest/fbcode/fbcodefile2_ovrsource
  A arvr-legacy/Research/researchfile2_ovrsource
  A arvr/arvrfile2_ovrsource

run sync in the tail mode over the complex fbsource changes
  $ mononoke_x_repo_sync_once 1 0 megarepo_master tail fbsource_master_newer --last-synced-commit fbsource_master --catch-up-once 2>&1 | grep "synced as" | wc -l
  7

now make those synced commits have all the interesting data
  $ cd $TESTTMP/meg-hg-cnt
  $ REPONAME=meg-mon hgmn -q pull
-- this commit did "createfile fbcode/fbcodefile3_fbsource"
  $ REPONAME=meg-mon hgmn -q status --copies --change megarepo_master~6
  A fbcode/fbcodefile3_fbsource
-- this commit did "cp fbcode/fbcodefile3_fbsource fbcode/fbcodefile3_copy_fbsource"
  $ REPONAME=meg-mon hgmn -q status --copies --change megarepo_master~5
  A fbcode/fbcodefile3_copy_fbsource
    fbcode/fbcodefile3_fbsource
-- this commit did "cp arvr/arvrfile_fbsource arvr/arvrfile_copy_fbsource"
  $ REPONAME=meg-mon hgmn -q status --copies --change megarepo_master~4
  A .fbsource-rest/arvr/arvrfile_copy_fbsource
    .fbsource-rest/arvr/arvrfile_fbsource
-- this commit did "cp arvr/arvrfile_fbsource fbcode/arvrfile_copy_fbsource"
  $ REPONAME=meg-mon hgmn -q status --copies --change megarepo_master~3
  A fbcode/arvrfile_copy_fbsource
    .fbsource-rest/arvr/arvrfile_fbsource
-- this commit did "cp fbcode/fbcodefile_fbsource arvr/fbcodefile_fbsource"
  $ REPONAME=meg-mon hgmn -q status --copies --change megarepo_master~2
  A .fbsource-rest/arvr/fbcodefile_fbsource
    fbcode/fbcodefile_fbsource
-- this commit did "rm arvr/fbcodefile_fbsource"
  $ REPONAME=meg-mon hgmn -q status --copies --change megarepo_master~1
  R .fbsource-rest/arvr/fbcodefile_fbsource
-- this commit did "rm fbcode/arvrfile_copy_fbsource"
  $ REPONAME=meg-mon hgmn -q status --copies --change megarepo_master
  R fbcode/arvrfile_copy_fbsource

let us test the auto-detection of last processed changeset
  $ mononoke_x_repo_sync_once 1 0 megarepo_master tail fbsource_master_newest --catch-up-once
  * using repo "fbs-mon" repoid RepositoryId(1) (glob)
  * using repo "meg-mon" repoid RepositoryId(0) (glob)
  * Got the last synced changesed: * (glob)
  * Starting a single tailing iteration from * (glob)
  * resolved tailed bookmark position to Some(ChangesetId(Blake2(*))) (glob)
  * Sanity checking ancestorship between * and fbsource_master_newest (glob)
  * Starting executing RangeNodeStream between * and * (glob)
  * Done executing RangeNodeStream. Found 2 changesets (glob)
  * Checking if * is already synced 1->0 (glob)
  * stopping tailing (glob)
  * Done preparing * (glob)
  * synced as * in *ms (glob)
  * Queue size is 0 (glob)


let us test that auto-detection works on a caught-up repo
  $ mononoke_x_repo_sync_once 1 0 megarepo_master tail fbsource_master_newest --catch-up-once
  * using repo "fbs-mon" repoid RepositoryId(1) (glob)
  * using repo "meg-mon" repoid RepositoryId(0) (glob)
  * Got the last synced changesed: * (glob)
  * Starting a single tailing iteration from * (glob)
  * resolved tailed bookmark position to Some(ChangesetId(Blake2(*))) (glob)
  * Sanity checking ancestorship between * and fbsource_master_newest (glob)
  * stopping tailing (glob)

let us make sure that tailing cannot start with a non-ancestor of a tailed bookmark
  $ mononoke_x_repo_sync_once 1 0 megarepo_master tail fbsource_master_newer --last-synced-commit fbsource_branch
  * using repo "fbs-mon" repoid RepositoryId(1) (glob)
  * using repo "meg-mon" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Got the last synced changesed: * (glob)
  * Starting a single tailing iteration from * (glob)
  * resolved tailed bookmark position to Some(ChangesetId(Blake2(*))) (glob)
  * Sanity checking ancestorship between * and fbsource_master_newer (glob)
  * Last processed node * is not an ancestor of tailed bookmark fbsource_master_newer (glob)
  [1]

test that synced commits are appropriately marked as public
  $ cd $TESTTMP/meg-hg-cnt
  $ hg log -T "{desc} {phase}\n" -r "all()"
  megarepo commit 1 public
  fbsource commit 2 public
  ovrsource commit 2 public
  fbsource commit 3 public
  fbsource commit 4 (with copy of preserved path into preserved path) public
  fbsource commit 5 (with copy of moved path into moved path) public
  fbsource commit 6 (with copy of moved path into preserved path) public
  fbsource commit 7 (with copy of preserved path into moved path) public
  fbsource commit 8 (with removal of a moved path) public
  fbsource commit 9 (with removal of a preserved path) public
