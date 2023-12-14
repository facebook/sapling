# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export COMMIT_SCRIBE_CATEGORY=mononoke_commits
  $ export BOOKMARK_SCRIBE_CATEGORY=mononoke_bookmark
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ LARGE_REPO_ID="0"
  $ SMALL_REPO_ID="1"
  $ IMPORTED_REPO_ID="2"
  $ IMPORTED_REPO_NAME="imported_repo"
  $ REPOID="$IMPORTED_REPO_ID" REPONAME="$IMPORTED_REPO_NAME" setup_common_config "blob_files"
  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
  >     "allow_change_xrepo_mapping_extra": true
  >   }
  > }
  > EOF
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase =
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
  >    }
  >   }
  > }
  > EOF

-- Init Mononoke thingies
  $ init_large_small_repo
  Adding synced mapping entry
  Starting Mononoke server

-- Start up the backsyncer in the background
  $ backsync_large_to_small_forever

Before config change
-- push to a large repo
  $ cd "$TESTTMP"/large-hg-client
  $ REPONAME=large-mon hgmn up -q master_bookmark

  $ mkdir -p smallrepofolder
  $ echo bla > smallrepofolder/bla
  $ hg ci -Aqm "before merge"
  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q
  $ log -r master_bookmark
  @  before merge [public;rev=3;dcd2b2ad78ed] default/master_bookmark
  │
  ~

-- wait a second to give backsyncer some time to catch up
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"

-- check the same commit in the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ log -r master_bookmark
  @  before merge [public;rev=2;*] default/master_bookmark (glob)
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
  > # bookmark: IC master_bookmark
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
  $ log -r master_bookmark
  @    [MEGAREPO GRADUAL MERGE] gradual merge (0) [public;rev=7;*] default/master_bookmark (glob)
  ├─╮
  │ │
  ~ ~

-- push to a large repo on top of merge
  $ mkdir -p smallrepofolder
  $ echo baz > smallrepofolder/baz
  $ hg ci -Aqm "after merge"
  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q
  $ log -r master_bookmark
  @  after merge [public;rev=*;*] default/master_bookmark (glob)
  │
  ~
-- wait a second to give backsyncer some time to catch up
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"

-- check the same commit in the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ log -r master_bookmark^::master_bookmark
  @  after merge [public;rev=*;*] default/master_bookmark (glob)
  │
  o  [MEGAREPO GRADUAL MERGE] gradual merge (0) [public;rev=3;*] (glob)
  │
  ~

  $ echo baz_from_small > baz
  $ hg ci -Aqm "after merge from small"
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark -q
  $ log -r master_bookmark^::master_bookmark
  @  after merge from small [public;rev=5;*] default/master_bookmark (glob)
  │
  o  after merge [public;rev=4;*] (glob)
  │
  ~
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ log -r master_bookmark^::master_bookmark
  o  after merge from small [public;rev=9;*] default/master_bookmark (glob)
  │
  @  after merge [public;rev=8;*] (glob)
  │
  ~


  $ testtool_drawdag -R "$IMPORTED_REPO_NAME" <<EOF
  > IC-ID
  > # exists: IC $IC
  > # bookmark: ID master_bookmark
  > EOF
  IC=65f0b76c034d87adf7dac6f0b5a5442ab3f62edda21adb8e8ec57d1a99fb5905
  ID=a14dee507f7605083e9a99901971ac7c5558d8b28d7d01090bd2cff2432fa707

  $ testtool_drawdag -R "$IMPORTED_REPO_NAME" <<EOF
  > ID-IE
  > # exists: ID $ID
  > # bookmark: IE master_bookmark
  > EOF
  ID=a14dee507f7605083e9a99901971ac7c5558d8b28d7d01090bd2cff2432fa707
  IE=ee275b10c734fa09ff52acf808a3baafd24348114fa937e8f41958490b9b6857

  $ testtool_drawdag -R "$IMPORTED_REPO_NAME" <<EOF
  > IE-IF-IG
  > # exists: IE $IE
  > # bookmark: IG master_bookmark
  > EOF
  IE=ee275b10c734fa09ff52acf808a3baafd24348114fa937e8f41958490b9b6857
  IF=20d91840623a3e0e6f3bc3c46ce6755d5f4c9ce6cfb49dae7b9cc8d9d0acfae9
  IG=2daec24778b88c326d1ba0f830d43a2d24d471dc22c48c8307096d0f60c9477f
