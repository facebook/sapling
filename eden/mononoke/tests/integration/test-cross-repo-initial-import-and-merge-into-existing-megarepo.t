# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ export COMMIT_SCRIBE_CATEGORY=mononoke_commits
  $ export BOOKMARK_SCRIBE_CATEGORY=mononoke_bookmark
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ LARGE_REPO_ID="0"
  $ SMALL_REPO_ID="1"
  $ IMPORTED_REPO_ID="2"
  $ ANOTHER_REPO_ID="3"
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase =
  > globalrevs =
  > EOF
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:cross_repo_skip_backsyncing_ordinary_empty_commits": true
  >   }
  > }
  > EOF

  $ setup_configerator_configs
  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "1": {
  >      "draft_push": false,
  >      "public_push": true
  >    },
  >   "2": {
  >      "draft_push": false,
  >      "public_push": false
  >    },
  >   "3": {
  >      "draft_push": false,
  >      "public_push": false
  >    }
  >   }
  > }
  > EOF

-- Init the imported repos
  $ IMPORTED_REPO_NAME="imported_repo"
  $ REPOID="$IMPORTED_REPO_ID" REPONAME="$IMPORTED_REPO_NAME" setup_common_config "blob_files"
  $ ANOTHER_REPO_NAME="another_repo"
  $ REPOID="$ANOTHER_REPO_ID" REPONAME="$ANOTHER_REPO_NAME" setup_common_config "blob_files"

-- Init large and small repos
  $ GLOBALREVS_PUBLISHING_BOOKMARK=master_bookmark GLOBALREVS_SMALL_REPO_ID=$SMALL_REPO_ID REPOID=$LARGE_REPO_ID INFINITEPUSH_ALLOW_WRITES=true REPONAME=large-mon setup_common_config blob_files
  $ DISALLOW_NON_PUSHREBASE=1 GLOBALREVS_PUBLISHING_BOOKMARK=master_bookmark REPOID=$SMALL_REPO_ID REPONAME=small-mon setup_common_config blob_files
  $ large_small_megarepo_config
  $ large_small_setup
  Adding synced mapping entry
  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

-- Start up the backsyncer in the background
  $ backsync_large_to_small_forever
-- Setup initial globalrevs
  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ quiet testtool_drawdag -R large-mon --no-default-files <<EOF
  > L_C-L_D
  > # extra: L_D  global_rev "1000157970"
  > # modify: L_D smallrepofolder/file.txt "22\n"
  > # bookmark: L_D master_bookmark
  > # exists: L_C $LARGE_MASTER_BONSAI
  > EOF
  $ set_bonsai_globalrev_mapping "$LARGE_REPO_ID" "$L_D" 1000157970
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"

Before config change
-- push to a large repo
  $ cd "$TESTTMP"/large-hg-client
  $ REPONAME=large-mon hgmn up -q master_bookmark

  $ mkdir -p smallrepofolder
  $ echo bla > smallrepofolder/bla
  $ hg ci -Aqm "before merge"
  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q
  $ log_globalrev -r master_bookmark
  o  before merge [public;globalrev=1000157971;a94d137602c0] default/master_bookmark
  │
  ~

-- wait a second to give backsyncer some time to catch up
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"

-- check the same commit in the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ log_globalrev -r master_bookmark
  @  before merge [public;globalrev=1000157971;61807722d4ec] default/master_bookmark
  │
  ~
  $ hg log -r master_bookmark -T "{files % '{file}\n'}"
  bla

-- config change
  $ update_commit_sync_map_for_new_repo_import

-- let LiveCommitSyncConfig pick up the changes
  $ force_update_configerator
-- populated imported repo that we're going to merge in
  $ testtool_drawdag -R "$IMPORTED_REPO_NAME" --no-default-files <<EOF
  > IA-IB-IC
  > # modify: IA "foo/a.txt" "creating foo directory"
  > # modify: IA "bar/b.txt" "creating bar directory"
  > # modify: IB "bar/c.txt" "random change"
  > # modify: IB "foo/d" "another random change"
  > # copy: IC "foo/b.txt" "copying file from bar into foo" IB "bar/b.txt"
  > # bookmark: IC heads/master_bookmark
  > EOF
  IA=84c956fabb06e66011b9ad0c8f12a17995b86d66b949ebb08a320d91b6ab7646
  IB=e1238541007d381b788b0aaab2425ed3aad02e38afd80b4e85bb922deb452972
  IC=65f0b76c034d87adf7dac6f0b5a5442ab3f62edda21adb8e8ec57d1a99fb5905

  $ with_stripped_logs mononoke_x_repo_sync "$IMPORTED_REPO_ID"  "$LARGE_REPO_ID" initial-import -i "$IC" --version-name "imported_noop"
  Starting session with id * (glob)
  Checking if 65f0b76c034d87adf7dac6f0b5a5442ab3f62edda21adb8e8ec57d1a99fb5905 is already synced 2->0
  syncing 65f0b76c034d87adf7dac6f0b5a5442ab3f62edda21adb8e8ec57d1a99fb5905
  Found 3 unsynced ancestors
  changeset 65f0b76c034d87adf7dac6f0b5a5442ab3f62edda21adb8e8ec57d1a99fb5905 synced as ecc8ec74d00988653ae64ebf206a9ed42898449125b91f59ecd1d8a0a93f4a97 in *ms (glob)
  successful sync of head 65f0b76c034d87adf7dac6f0b5a5442ab3f62edda21adb8e8ec57d1a99fb5905

  $ mononoke_newadmin fetch -R large-mon  -i ecc8ec74d00988653ae64ebf206a9ed42898449125b91f59ecd1d8a0a93f4a97 --json | jq .parents
  [
    "fa5173cebb32a908f52fd6f01b442a76f013bda5b3d4bbcf3e29af0227bbb74f"
  ]

-- use gradual merge to merge in just one commit (usually this one does sequence of merges)
  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ COMMIT_DATE="1985-09-04T00:00:00.00Z"
  $ with_stripped_logs quiet_grep merging -- megarepo_tool gradual-merge \
  > test_user \
  > "gradual merge" \
  > --pre-deletion-commit fa5173cebb32a908f52fd6f01b442a76f013bda5b3d4bbcf3e29af0227bbb74f \
  > --last-deletion-commit ecc8ec74d00988653ae64ebf206a9ed42898449125b91f59ecd1d8a0a93f4a97 \
  > --bookmark master_bookmark \
  > --limit 1 \
  > --commit-date-rfc3339 "$COMMIT_DATE"
  merging 1 commits

-- wait a second to give backsyncer some time to catch up
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"

-- check that merge has made into large repo
  $ cd "$TESTTMP"/large-hg-client
  $ REPONAME=large-mon hgmn -q pull
  $ REPONAME=large-mon hgmn up -q master_bookmark
  $ log_globalrev -r master_bookmark
  @    [MEGAREPO GRADUAL MERGE] gradual merge (0) [public;globalrev=1000157972;9af7a2bbf0f5] default/master_bookmark
  ├─╮
  │ │
  ~ ~

-- push to a large repo on top of merge
  $ mkdir -p smallrepofolder
  $ echo baz > smallrepofolder/baz
  $ hg ci -Aqm "after merge"
  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q
  $ log_globalrev -r master_bookmark
  o  after merge [public;globalrev=1000157973;1220098b4cde] default/master_bookmark
  │
  ~
-- wait a second to give backsyncer some time to catch up
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"

-- check the same commit in the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ log_globalrev -r master_bookmark^::master_bookmark
  @  after merge [public;globalrev=1000157973;3381b75593e5] default/master_bookmark
  │
  o  [MEGAREPO GRADUAL MERGE] gradual merge (0) [public;globalrev=1000157972;9351f7816915]
  │
  ~

  $ echo baz_from_small > baz
  $ hg ci -Aqm "after merge from small"
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark -q
  $ log_globalrev -r master_bookmark^::master_bookmark
  o  after merge from small [public;globalrev=1000157974;c17052372d27] default/master_bookmark
  │
  o  after merge [public;globalrev=1000157973;3381b75593e5]
  │
  ~
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ log_globalrev -r master_bookmark^::master_bookmark
  o  after merge from small [public;globalrev=1000157974;4d44ba9e1ca3] default/master_bookmark
  │
  o  after merge [public;globalrev=1000157973;1220098b4cde]
  │
  ~


  $ testtool_drawdag -R "$IMPORTED_REPO_NAME" <<EOF
  > IC-ID
  > # exists: IC $IC
  > # bookmark: ID master_bookmark
  > EOF
  IC=65f0b76c034d87adf7dac6f0b5a5442ab3f62edda21adb8e8ec57d1a99fb5905
  ID=a14dee507f7605083e9a99901971ac7c5558d8b28d7d01090bd2cff2432fa707

  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ with_stripped_logs mononoke_x_repo_sync "$IMPORTED_REPO_ID"  "$LARGE_REPO_ID" once --commit "$ID" --unsafe-change-version-to "new_version" --target-bookmark master_bookmark
  Starting session with id * (glob)
  changeset resolved as: ChangesetId(Blake2(a14dee507f7605083e9a99901971ac7c5558d8b28d7d01090bd2cff2432fa707))
  Checking if a14dee507f7605083e9a99901971ac7c5558d8b28d7d01090bd2cff2432fa707 is already synced 2->0
  1 unsynced ancestors of a14dee507f7605083e9a99901971ac7c5558d8b28d7d01090bd2cff2432fa707
  syncing a14dee507f7605083e9a99901971ac7c5558d8b28d7d01090bd2cff2432fa707 via pushrebase for master_bookmark
  changeset a14dee507f7605083e9a99901971ac7c5558d8b28d7d01090bd2cff2432fa707 synced as 402c52f0f2156a83bf5354aae35c3cae55e92b23da3ed61bc10ee7960e172c8e in *ms (glob)
  successful sync
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"

  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ hg update master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log_globalrev -r .^::.
  @  ID [public;globalrev=1000157975;8d707fde6f5e] default/master_bookmark
  │
  o  after merge from small [public;globalrev=1000157974;c17052372d27]
  │
  ~
  $ echo baz_from_small2 > bar
  $ hg add bar
  $ hg ci -Aqm "after mapping change from small"
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark -q

  $ log_globalrev -r master_bookmark^::master_bookmark
  o  after mapping change from small [public;globalrev=1000157976;ecca553b5690] default/master_bookmark
  │
  o  ID [public;globalrev=1000157975;8d707fde6f5e]
  │
  ~

  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ log_globalrev -r master_bookmark^::master_bookmark
  o  after mapping change from small [public;globalrev=1000157976;54bd67a132c8] default/master_bookmark
  │
  o  ID [public;globalrev=1000157975;4f56877f458b]
  │
  ~

  $ testtool_drawdag -R "$IMPORTED_REPO_NAME" <<EOF
  > ID-IE
  > # exists: ID $ID
  > # bookmark: IE heads/master_bookmark
  > EOF
  ID=a14dee507f7605083e9a99901971ac7c5558d8b28d7d01090bd2cff2432fa707
  IE=ee275b10c734fa09ff52acf808a3baafd24348114fa937e8f41958490b9b6857

  $ testtool_drawdag -R "$IMPORTED_REPO_NAME" <<EOF
  > IE-IF-IG
  > # exists: IE $IE
  > # bookmark: IG heads/master_bookmark
  > EOF
  IE=ee275b10c734fa09ff52acf808a3baafd24348114fa937e8f41958490b9b6857
  IF=20d91840623a3e0e6f3bc3c46ce6755d5f4c9ce6cfb49dae7b9cc8d9d0acfae9
  IG=2daec24778b88c326d1ba0f830d43a2d24d471dc22c48c8307096d0f60c9477f
  $ quiet mononoke_newadmin mutable-counters --repo-id $LARGE_REPO_ID set xreposync_from_$IMPORTED_REPO_ID 2
  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi large-mon master_bookmark)
  $ quiet mononoke_x_repo_sync "$IMPORTED_REPO_ID"  "$LARGE_REPO_ID" tail --bookmark-regex "heads/master_bookmark" --catch-up-once
  $ wait_for_bookmark_move_away_edenapi large-mon master_bookmark  "$PREV_BOOK_VALUE"

  $ REPONAME=large-mon hgmn pull -q
  $ log_globalrev -r master_bookmark^^^::master_bookmark
  o  IG [public;globalrev=1000157979;0d969c3e772c] default/master_bookmark
  │
  o  IF [public;globalrev=1000157978;a3fc14316d38]
  │
  o  IE [public;globalrev=1000157977;4d7edff71de1]
  │
  o  after mapping change from small [public;globalrev=1000157976;54bd67a132c8]
  │
  ~

-- The rest of this repo merges in another repo into large repo
-- this covers the scenario where we have to do a merge and mapping change in large
-- repo while there's forward sync going on.
  $ testtool_drawdag -R "$ANOTHER_REPO_NAME" --no-default-files <<EOF
  > AA-AB-AC
  > # modify: AA "foo/a.txt" "creating foo directory"
  > # modify: AA "bar/b.txt" "creating bar directory"
  > # modify: AB "bar/c.txt" "random change"
  > # modify: AB "foo/d" "another random change"
  > # copy: AC "foo/b.txt" "copying file from bar into foo" AB "bar/b.txt"
  > # bookmark: AC heads/master_bookmark
  > EOF
  AA=be49ffcb679bb0485fd5cf5a05013e8554065d19796407d5ab97b556c951cd35
  AB=1bb09eba8700dd4438bf66006b0f70cce454b32adb9a168f1e8a2d04c19c0cd3
  AC=156943c35cda314d72b0177b06d5edf3c92dc9c9505d7b3171b9230f7c1768bb

  $ with_stripped_logs mononoke_x_repo_sync "$ANOTHER_REPO_ID"  "$LARGE_REPO_ID" initial-import -i "$AC" --version-name "another_noop"
  Starting session with id * (glob)
  Checking if 156943c35cda314d72b0177b06d5edf3c92dc9c9505d7b3171b9230f7c1768bb is already synced 3->0
  syncing 156943c35cda314d72b0177b06d5edf3c92dc9c9505d7b3171b9230f7c1768bb
  Found 3 unsynced ancestors
  changeset 156943c35cda314d72b0177b06d5edf3c92dc9c9505d7b3171b9230f7c1768bb synced as 0a9797a0fa6b3284b9d73ec43357f06a9b00d6fa402122d1bbfbeac16e3a2c39 in *ms (glob)
  successful sync of head 156943c35cda314d72b0177b06d5edf3c92dc9c9505d7b3171b9230f7c1768bb

  $ mononoke_newadmin fetch -R large-mon  -i 0a9797a0fa6b3284b9d73ec43357f06a9b00d6fa402122d1bbfbeac16e3a2c39 --json | jq .parents
  [
    "7b877236dc63b9df21954f78b6c8ce8b69a844e786fe58a2932de04ac685075d"
  ]

-- use gradual merge to merge in just one commit (usually this one does sequence of merges)
  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ COMMIT_DATE="2006-05-03T22:38:01.00Z"
  $ with_stripped_logs quiet_grep merging -- megarepo_tool gradual-merge \
  > test_user \
  > "another merge" \
  > --pre-deletion-commit 7b877236dc63b9df21954f78b6c8ce8b69a844e786fe58a2932de04ac685075d \
  > --last-deletion-commit 0a9797a0fa6b3284b9d73ec43357f06a9b00d6fa402122d1bbfbeac16e3a2c39 \
  > --bookmark master_bookmark \
  > --limit 1 \
  > --commit-date-rfc3339 "$COMMIT_DATE"
  merging 1 commits

-- wait a second to give backsyncer some time to catch up
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"

-- check that merge has made into large repo
  $ cd "$TESTTMP"/large-hg-client
  $ REPONAME=large-mon hgmn -q pull
  $ REPONAME=large-mon hgmn up -q master_bookmark
  $ log_globalrev -r master_bookmark
  @    [MEGAREPO GRADUAL MERGE] another merge (0) [public;globalrev=1000157980;c58d6329efff] default/master_bookmark
  ├─╮
  │ │
  ~ ~

  $ testtool_drawdag -R "$ANOTHER_REPO_NAME" <<EOF
  > AC-AD
  > # exists: AC $AC
  > # bookmark: AD master_bookmark
  > EOF
  AC=156943c35cda314d72b0177b06d5edf3c92dc9c9505d7b3171b9230f7c1768bb
  AD=1d0bbdb162c2887a5b93893d7a48fd852a304ab58be2245899bb795e80aa10e9

  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ with_stripped_logs mononoke_x_repo_sync "$ANOTHER_REPO_ID"  "$LARGE_REPO_ID" once --commit "$AD" --unsafe-change-version-to "another_version" --target-bookmark master_bookmark
  Starting session with id * (glob)
  changeset resolved as: ChangesetId(Blake2(1d0bbdb162c2887a5b93893d7a48fd852a304ab58be2245899bb795e80aa10e9))
  Checking if 1d0bbdb162c2887a5b93893d7a48fd852a304ab58be2245899bb795e80aa10e9 is already synced 3->0
  1 unsynced ancestors of 1d0bbdb162c2887a5b93893d7a48fd852a304ab58be2245899bb795e80aa10e9
  syncing 1d0bbdb162c2887a5b93893d7a48fd852a304ab58be2245899bb795e80aa10e9 via pushrebase for master_bookmark
  changeset 1d0bbdb162c2887a5b93893d7a48fd852a304ab58be2245899bb795e80aa10e9 synced as 76b08a5702ff09571621ca88b107d886963d2c8265f508edc6e4d8f95777fd3e in *ms (glob)
  successful sync
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"

  $ testtool_drawdag -R "$IMPORTED_REPO_NAME" <<EOF
  > IG-IH-II
  > # exists: IG $IG
  > # bookmark: II heads/master_bookmark
  > EOF
  IG=2daec24778b88c326d1ba0f830d43a2d24d471dc22c48c8307096d0f60c9477f
  IH=f5d5d65c2f874454ddf7b1b5da1e029ba0fc9258ded48c9cca73fb0d7c2df3cc
  II=09b37e2f0429911d6b00d1b129471b6da45b71b5f0ef4ba720ee7e97cea909a8

  $ with_stripped_logs mononoke_x_repo_sync "$IMPORTED_REPO_ID"  "$LARGE_REPO_ID" tail --bookmark-regex "heads/master_bookmark" --catch-up-once 
  Starting session with id * (glob)
  queue size is 1
  processing log entry #5
  2 unsynced ancestors of 09b37e2f0429911d6b00d1b129471b6da45b71b5f0ef4ba720ee7e97cea909a8
  force using mapping another_version to rewrite 76b08a5702ff09571621ca88b107d886963d2c8265f508edc6e4d8f95777fd3e
  syncing f5d5d65c2f874454ddf7b1b5da1e029ba0fc9258ded48c9cca73fb0d7c2df3cc via pushrebase for master_bookmark
  Syncing f5d5d65c2f874454ddf7b1b5da1e029ba0fc9258ded48c9cca73fb0d7c2df3cc failed in *ms: Pushrebase of synced commit failed - check config for overlaps: Error(version mismatch for forward synced commit: expected new_version, got another_version) (glob)
  failed to sync bookmark update log #5, Pushrebase of synced commit failed - check config for overlaps: Error(version mismatch for forward synced commit: expected new_version, got another_version)
  Execution error: Pushrebase of synced commit failed - check config for overlaps: Error(version mismatch for forward synced commit: expected new_version, got another_version)
  Error: Execution failed
