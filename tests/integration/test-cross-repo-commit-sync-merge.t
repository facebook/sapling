  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ REPOTYPE="blob_files"
  $ REPOID=0 REPONAME=meg-mon setup_common_config $REPOTYPE
  $ REPOID=1 REPONAME=fbs-mon setup_common_config $REPOTYPE
  $ setup_commitsyncmap
  $ ls $TESTTMP/monsql/sqlite_dbs
  ls: cannot access $TESTTMP/monsql/sqlite_dbs: No such file or directory
  [2]

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

  $ hg up -q null
  $ createfile arvr/tomerge
  $ hg -q ci -m "to merge"
  $ TOMERGE="$(hg log -r . -T '{node}')"

  $ hg up -q fbsource_c1
  $ hg up -q .
  $ hg merge -q "$TOMERGE"
  $ hg -q ci -m 'merge_commit' && hg book -ir . fbsource_master

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

get some bonsai hashes to avoid magic strings later
  $ FBSOURCE_C1_BONSAI=$(get_bonsai_bookmark 1 fbsource_c1)
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 1 fbsource_master)
  $ MEGAREPO_MERGE_BONSAI=$(get_bonsai_bookmark 0 megarepo_master)

setup hg client repos
  $ hgclone_treemanifest ssh://user@dummy/fbs-hg-srv fbs-hg-cnt --noupdate
  $ hgclone_treemanifest ssh://user@dummy/meg-hg-srv meg-hg-cnt --noupdate

start mononoke server
  $ mononoke
  $ wait_for_mononoke

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

run the sync again
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
  A .fbsource-rest/arvr/tomerge
  $ REPONAME=meg-mon hgmn status --change 4523b8346e49
  A .fbsource-rest/arvr/tomerge
  $ hg log -G
  o    changeset:   2:9c3b218de12e
  |\   bookmark:    megarepo_master
  | |  parent:      0:14e20a60e5f4
  | |  parent:      1:4523b8346e49
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merge_commit
  | |
  | o  changeset:   1:4523b8346e49
  |    parent:      -1:000000000000
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     to merge
  |
  o  changeset:   0:14e20a60e5f4
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     megarepo commit 1
  
