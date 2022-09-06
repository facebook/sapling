# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

Setup repositories
  $ REPOTYPE="blob_files"
  $ MEG_REPOID=0
  $ FBS_REPOID=1
  $ OVR_REPOID=2

  $ NO_BOOKMARKS_CACHE=1 REPOID=$MEG_REPOID REPONAME=meg-mon setup_common_config $REPOTYPE
  $ NO_BOOKMARKS_CACHE=1 REPOID=$FBS_REPOID REPONAME=fbs-mon setup_common_config $REPOTYPE
  $ NO_BOOKMARKS_CACHE=1 REPOID=$OVR_REPOID REPONAME=ovr-mon setup_common_config $REPOTYPE

  $ setup_commitsyncmap
  $ setup_configerator_configs
-- initial push-redirection setup redirects ovrsource into megarepo,
-- which is the large repo at this point
  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "2": {
  >      "draft_push": false,
  >      "public_push": true
  >    }
  >   }
  > }
  > EOF

  $ cat >> "$HGRCPATH" <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > pushrebase=
  > remotenames=
  > EOF

  $ function createfile { mkdir -p "$(dirname  $1)" && echo "$1" > "$1" && hg add -q "$1"; }
  $ function createfile_with_content { mkdir -p "$(dirname  $1)" && echo "$2" > "$1" && hg add -q "$1"; }

-- init hg fbsource server repo
  $ cd $TESTTMP
  $ hginit_treemanifest fbs-hg-srv
  $ cd fbs-hg-srv
-- create an initial commit, which will be the last_synced_commit
  $ createfile fbcode/fbcodefile_fbsource
  $ createfile arvr/arvrfile_fbsource
  $ createfile otherfile_fbsource
  $ hg -q ci -m "fbsource commit 1" && hg book -ir . master_bookmark

-- init hg ovrsource server repo
  $ cd $TESTTMP
  $ hginit_treemanifest ovr-hg-srv
  $ cd ovr-hg-srv
  $ createfile fbcode/fbcodefile_ovrsource
  $ createfile arvr/arvrfile_ovrsource
  $ createfile otherfile_ovrsource
  $ createfile Research/researchfile_ovrsource
  $ hg -q ci -m "ovrsource commit 1" && hg book -r . master_bookmark

-- init hg megarepo server repo
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

-- blobimport hg server repos into Mononoke repos
  $ cd "$TESTTMP"
  $ REPOID=$MEG_REPOID blobimport meg-hg-srv/.hg meg-mon
  $ REPOID=$FBS_REPOID blobimport fbs-hg-srv/.hg fbs-mon
  $ REPOID=$OVR_REPOID blobimport ovr-hg-srv/.hg ovr-mon

-- setup hg client repos
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/fbs-hg-srv fbs-hg-cnt --noupdate
  $ hgclone_treemanifest ssh://user@dummy/ovr-hg-srv ovr-hg-cnt --noupdate
  $ hgclone_treemanifest ssh://user@dummy/meg-hg-srv meg-hg-cnt --noupdate


Start mononoke server
  $ start_and_wait_for_mononoke_server

Setup commit sync mapping
-- get some bonsai hashes to avoid magic strings later
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark $FBS_REPOID master_bookmark)
  $ OVRSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark $OVR_REPOID master_bookmark)
  $ MEGAREPO_MERGE_BONSAI=$(get_bonsai_bookmark $MEG_REPOID master_bookmark)

-- insert sync mapping entry
  $ add_synced_commit_mapping_entry $FBS_REPOID $FBSOURCE_MASTER_BONSAI $MEG_REPOID $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME
  $ add_synced_commit_mapping_entry $OVR_REPOID $OVRSOURCE_MASTER_BONSAI $MEG_REPOID $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME
-- tell backsyncer that we're all caught up in ovrsource
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES ($OVR_REPOID, 'backsync_from_$MEG_REPOID', 3)";


Perform ovrsource pushrebase, make sure it is push-redirected into Megarepo
  $ cd "$TESTTMP/ovr-hg-cnt"
  $ REPONAME=ovr-mon hgmn up -q master_bookmark
  $ echo 1 > pushredirected_1 && hg addremove -q && hg ci -q -m pushredirected_1
  $ REPONAME=ovr-mon hgmn push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark
-- pushredirected_1 was correctly pushed to master_bookmark in ovrsource
  $ log -r master_bookmark
  @  pushredirected_1 [public;rev=1;bb12ff0dc64f] default/master_bookmark
  │
  ~
-- pushredirected_1 is also present in megarepo
  $ cd "$TESTTMP"/meg-hg-cnt
  $ REPONAME=meg-mon hgmn pull -q
  $ log -r master_bookmark
  o  pushredirected_1 [public;rev=1;4358fa9b678c] default/master_bookmark
  │
  ~
-- ensure that ovrsource root path ends up in megarepo's arvr-legacy
  $ REPONAME=meg-mon hgmn up master_bookmark -q
  $ ls arvr-legacy | grep pushredirected
  pushredirected_1
- compare the working copies
  $ REPOIDLARGE=$MEG_REPOID REPOIDSMALL=$OVR_REPOID verify_wc master_bookmark

  $ cd "$TESTTMP/ovr-hg-cnt"
  $ REPONAME=ovr-mon hgmn up -q master_bookmark
  $ echo 2 > pushredirected_2 && hg addremove -q && hg ci -q -m pushredirected_2
  $ REPONAME=ovr-mon hgmn push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark
-- pushredirected_2 was correctly pushed to master_bookmark
  $ log -r master_bookmark
  @  pushredirected_2 [public;rev=2;2d72ff1821dd] default/master_bookmark
  │
  ~
-- pushredirected_2 is also present in the megarepo
  $ cd "$TESTTMP"/meg-hg-cnt
  $ REPONAME=meg-mon hgmn pull -q
  $ log -r master_bookmark
  o  pushredirected_2 [public;rev=2;538143697725] default/master_bookmark
  │
  ~
-- let's see what's where in megarepo
  $ REPONAME=meg-mon hgmn up master_bookmark -q
  $ ls arvr-legacy | grep pushredirected
  pushredirected_1
  pushredirected_2
- compare the working copies
  $ REPOIDLARGE=$MEG_REPOID REPOIDSMALL=$OVR_REPOID verify_wc master_bookmark


Set current version of CommitSyncConfig to have fbsource as large repo,
but disable push-redirection until invisible merge is done
-- stop mononoke before changing config with large repo change
  $ killandwait $MONONOKE_PID

Add a new config version to "all" configs, this new version has fbsource as large repo.
  $ cp "$TEST_FIXTURES/commitsync/all_with_flipped_config.json" "$COMMIT_SYNC_CONF/all"

-- This is an expected state of our configs at the last restart before
-- the invisible merge
  $ cp "$TEST_FIXTURES/commitsync/flipped_config.json" "$COMMIT_SYNC_CONF/current"
  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "2": {
  >      "draft_push": false,
  >      "public_push": false
  >    }
  >   }
  > }
  > EOF
  $ cp "$TEST_FIXTURES/commitsync/flipped_config.toml" "$TESTTMP/mononoke-config/common/commitsyncmap.toml"
-- start mononoke
  $ mononoke
  $ wait_for_mononoke


Prepare for the invisible merge
1. Create an independent ovrsource DAG in fbsource
  $ cd "$TESTTMP/ovr-hg-cnt"
  $ REPONAME=fbs-mon hgmn push -q \
  >     --config extensions.pushrebase=! \
  >     --to ovrsource/pre_move_master \
  >     --create --force -r . \
  >     mononoke://$(mononoke_address)/fbs-mon
  warning: repository is unrelated
1.5. Mark independent ovrsource DAG in fbsource as preserved
  $ cd "$TESTTMP"
  $ hg log -T "{node}\n" --cwd "ovr-hg-cnt" \
  > | xargs -I {} sqlite3 monsql/sqlite_dbs "SELECT HEX(bcs_id) FROM bonsai_hg_mapping WHERE hg_cs_id = X'{}'" \
  > | sort \
  > | uniq \
  > | xargs -I {} sqlite3 monsql/sqlite_dbs "INSERT INTO synced_commit_mapping (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id) VALUES ($OVR_REPOID, X'{}', $FBS_REPOID, X'{}')"

2. Move files on top of the intermediate DAG
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ REPONAME=fbs-mon hgmn pull -q
  $ REPONAME=fbs-mon hgmn up -q ovrsource/pre_move_master
  $ mkdir arvr-legacy .ovrsource-rest
  $ hg mv fbcode .ovrsource-rest/
  moving fbcode/fbcodefile_ovrsource to .ovrsource-rest/fbcode/fbcodefile_ovrsource
  $ hg mv arvr .ovrsource-rest/arvr
  moving arvr/arvrfile_ovrsource to .ovrsource-rest/arvr/arvrfile_ovrsource
  $ hg mv otherfile_ovrsource pushredirected_1 pushredirected_2 Research arvr-legacy/
  moving Research/researchfile_ovrsource to arvr-legacy/Research/researchfile_ovrsource
  $ REPONAME=fbs-mon hgmn ci -m "move ovrsource files into place"
  $ REPONAME=fbs-mon hgmn -q push --to ovrsource/moved_master --create
3. Implement a gradual merge policy
  $ COMMIT_DATE="1985-09-04T00:00:00.00Z"
  $ cd "$TESTTMP"
  $ REPOID=$FBS_REPOID megarepo_tool pre-merge-delete \
  > 2>/dev/null \
  >  ovrsource/moved_master \
  >  author "merge preparation" \
  >  --even-chunk-size 2 \
  > --commit-date-rfc3339 "$COMMIT_DATE"
  69cb4dfdec92e3b84ee1c5b064c33ac3fb980bdae0d6fba7a2a75cb0670af599
  15f0ba01db155d0431552defe999ca51e5b8a0a632bc323cc2faeeddc4064cc1
  $ get_bonsai_bookmark $FBS_REPOID ovrsource/moved_master
  0b114e8a3d0d62a31ff8f99b8894603cf37cdb6edc070d744a7a457bd360fc0a
-- a list of commits we want to merge also includes the pre-delete commit
  $ TOMERGES=(69cb4dfdec92e3b84ee1c5b064c33ac3fb980bdae0d6fba7a2a75cb0670af599 15f0ba01db155d0431552defe999ca51e5b8a0a632bc323cc2faeeddc4064cc1 0b114e8a3d0d62a31ff8f99b8894603cf37cdb6edc070d744a7a457bd360fc0a)
-- calculate to-merge working copy sizes, they should be gradually increasing
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ for TOMERGE in "${TOMERGES[@]}"; do
  >  HGHASH=$(REPOID=$FBS_REPOID mononoke_admin --log-level=ERROR convert --from bonsai --to hg $TOMERGE)
  >  REPONAME=fbs-mon hgmn up -q $HGHASH
  >  FILECOUNT=$(find . -path ./.hg -prune -o -type f -print | wc -l)
  >  echo "$HGHASH: $FILECOUNT files"
  > done
  0bcd370350f8ffa50b01a71ebde58685eb8a48c4: 2 files
  7ed84adf14359250fce47e358e02da84a69432d3: 4 files
  1bb93fce182b04f42c237baaea017ed96becdc72: 6 files

Do the invisible merge by gradually merging TOMERGES into master
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ REPONAME=fbs-mon hgmn up -q master_bookmark
  $ MASTER_BEFORE_MERGES=$(hg log -r . -T "{node}")
  $ for TOMERGE in "${TOMERGES[@]}"; do
  >  CURRENT=$(hg log -r . -T "{node}")
  >  echo "Current: $CURRENT"
  >  echo "To merge: $TOMERGE"
  >  MERGE=$(REPOID=$FBS_REPOID megarepo_tool --log-level=ERROR bonsai-merge $CURRENT $TOMERGE author "merge execution" --commit-date-rfc3339 "$COMMIT_DATE")
  >  HGMERGE=$(REPOID=$FBS_REPOID mononoke_admin --log-level=ERROR convert --from bonsai --to hg $MERGE)
  >  echo "Merged as (bonsai): $MERGE"
  >  echo "Merged as (hg): $HGMERGE"
  >  REPONAME=fbs-mon hgmn up -q $HGMERGE
  >  FILECOUNT_1=$([ -d ./.ovrsource-rest ] && find ./.ovrsource-rest -type f | wc -l)
  >  FILECOUNT_2=$([ -d ./arvr-legacy ] && find ./arvr-legacy -type f | wc -l)
  >  FILECOUNT=$(($FILECOUNT_1 + $FILECOUNT_2))
  >  echo "file count is: $FILECOUNT"
  >  REPOID=$FBS_REPOID mononoke_admin --log-level=ERROR bookmarks set master_bookmark $HGMERGE
  >  flush_mononoke_bookmarks
  >  echo "intermediate" >> fbcode/fbcodefile_fbsource
  >  REPONAME=fbs-mon hgmn debugmakepublic -r .
  >  REPONAME=fbs-mon hgmn ci -qm "intermediate commit between gradual merge commits"
  >  REPONAME=fbs-mon hgmn push -q --to master_bookmark
  > done
  Current: cb536a1a0bd5e1e5226a09530ab95ae790b717d7
  To merge: 69cb4dfdec92e3b84ee1c5b064c33ac3fb980bdae0d6fba7a2a75cb0670af599
  Merged as (bonsai): 69cc63c9e59f7cc7b6daa0c36832489632674e722a9364ff2712919af93109d7
  Merged as (hg): 91d643697945d5bb502a2c1c2f75ec36b855f308
  file count is: 2
  Current: 51c49b0bd6828234ce57148769ca56f254e463bd
  To merge: 15f0ba01db155d0431552defe999ca51e5b8a0a632bc323cc2faeeddc4064cc1
  Merged as (bonsai): e0d0f35215c77449e9e63807cbec7f09368ebd0591f11adb53a133a53add4a7a
  Merged as (hg): f2ac779eb5ef342aab788bcb278e57e53b2bc83e
  file count is: 4
  Current: 0eb9c5feca13f5b7c5daf2c34b659c3846569fad
  To merge: 0b114e8a3d0d62a31ff8f99b8894603cf37cdb6edc070d744a7a457bd360fc0a
  Merged as (bonsai): 9e0bfdd3a6cd0a41697e67f00baf3d060e0e7660f7e2b0e2be34c3f5c5691984
  Merged as (hg): 005686fbc230dc0be4e1cc2fabf46d87bbb19001
  file count is: 6
  $ REPONAME=fbs-mon hgmn pull -q
  $ hg log -r "$MASTER_BEFORE_MERGES::master_bookmark" -T "{phase} {desc|firstline}\n"
  public fbsource commit 1
  public merge execution
  public intermediate commit between gradual merge commits
  public merge execution
  public intermediate commit between gradual merge commits
  public merge execution
  public intermediate commit between gradual merge commits


Create special marker commits in both repos, which can be just marked as rewritten into each other
  $ cd "$TESTTMP/ovr-hg-cnt"
  $ hg ci -qm "pre push-redirection marker" --config ui.allowemptycommit=True
  $ REPONAME=ovr-mon hgmn push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ hg ci -qm "pre push-redirection marker" --config ui.allowemptycommit=True
  $ REPONAME=fbs-mon hgmn push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark

Set mutable counter for the backsyncer (we've synced everything up until now)
  $ LATEST_LOG_ENTRY_ID=$(sqlite3 $TESTTMP/monsql/sqlite_dbs "SELECT MAX(id) FROM bookmarks_update_log WHERE repo_id = $FBS_REPOID")
  $ sqlite3 $TESTTMP/monsql/sqlite_dbs "INSERT INTO mutable_counters (repo_id, name, value) VALUES ($OVR_REPOID, 'backsync_from_$FBS_REPOID', $LATEST_LOG_ENTRY_ID)"

Set working copy equivalence between ovrsource master and fbsource master
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark $FBS_REPOID master_bookmark)
  $ OVRSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark $OVR_REPOID master_bookmark)
  $ sqlite3 $TESTTMP/monsql/sqlite_dbs \
  > "INSERT INTO synced_working_copy_equivalence \
  >    (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id, sync_map_version_name) \
  >  VALUES \
  >    ($OVR_REPOID, X'$OVRSOURCE_MASTER_BONSAI', $FBS_REPOID, X'$FBSOURCE_MASTER_BONSAI', 'TEST_VERSION_NAME_FLIPPED')"

Set current version of CommitSyncConfig to be push-redirecting ovrsource
into fbsource
  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "2": {
  >      "draft_push": false,
  >      "public_push": true
  >    }
  >   }
  > }
  > EOF
  $ force_update_configerator

Perform ovrsource pushrebase, make sure it is push-redirected into Fbsource
  $ cd "$TESTTMP/ovr-hg-cnt"
  $ REPONAME=ovr-mon hgmn up -q master_bookmark
  $ echo 1 > pushredirected_3 && hg addremove -q && hg ci -q -m pushredirected_3
  $ REPONAME=ovr-mon hgmn push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark
-- pushredirected_3 was correctly pushed to master_bookmark in ovrsource
  $ log -r master_bookmark
  @  pushredirected_3 [public;rev=5;4355e6b9eafb] default/master_bookmark
  │
  ~
-- make the bookmark change visible to other repos. the cache invalidates
-- itself on push but not across repos.
  $ flush_mononoke_bookmarks
-- pushredirected_3 is also present in megarepo
  $ cd "$TESTTMP"/fbs-hg-cnt
  $ REPONAME=fbs-mon hgmn pull -q
  $ log -r master_bookmark
  o  pushredirected_3 [public;rev=14;223e2529a7b8] default/master_bookmark
  │
  ~
-- ensure that ovrsource root path ends up in megarepo's arvr-legacy
  $ REPONAME=fbs-mon hgmn up master_bookmark -q
  $ ls arvr-legacy | grep pushredirected_3
  pushredirected_3
- compare the working copies
  $ REPOIDLARGE=$FBS_REPOID REPOIDSMALL=$OVR_REPOID verify_wc master_bookmark
