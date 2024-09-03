# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export LARGE_REPO_ID=0
  $ export SMALL_REPO_ID=1

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

setup configuration
  $ REPOTYPE="blob_files"
  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 REPOID=$LARGE_REPO_ID REPONAME=large-mon setup_common_config $REPOTYPE
  $ ENABLE_API_WRITES=1 REPOID=$SMALL_REPO_ID REPONAME=small-mon-1 setup_common_config $REPOTYPE

  $ cat >> "$TESTTMP/mononoke-config/common/commitsyncmap.toml" <<EOF
  > [megarepo_test]
  > large_repo_id = 0
  > common_pushrebase_bookmarks = ["master_bookmark"]
  >   [[megarepo_test.small_repos]]
  >   repoid = 1
  >   bookmark_prefix = "bookprefix1/"
  >   default_action = "prepend_prefix"
  >   default_prefix = "smallrepofolder1"
  >   direction = "large_to_small"
  >      [megarepo_test.small_repos.mapping]
  >      "special"="specialsmallrepofolder1"
  > EOF

setup configerator configs
  $ setup_configerator_configs
  $ enable_pushredirect 1
  $ cat > "$COMMIT_SYNC_CONF/current" <<EOF
  > {
  >   "repos": {
  >     "megarepo_test": {
  >         "large_repo_id": 0,
  >         "common_pushrebase_bookmarks": [
  >           "master_bookmark"
  >         ],
  >         "small_repos": [
  >           {
  >             "repoid": 1,
  >             "default_action": "prepend_prefix",
  >             "default_prefix": "smallrepofolder1",
  >             "bookmark_prefix": "bookprefix1/",
  >             "mapping": {
  >               "special": "specialsmallrepofolder1"
  >             },
  >             "direction": "large_to_small"
  >           }
  >         ],
  >         "version_name": "TEST_VERSION_NAME_LIVE_V1"
  >     }
  >   }
  > }
  > EOF
  $ cat > "$COMMIT_SYNC_CONF/all" << EOF
  > {
  >  "repos": {
  >    "megarepo_test": {
  >      "versions": [
  >        {
  >          "large_repo_id": 0,
  >          "common_pushrebase_bookmarks": ["master_bookmark"],
  >          "small_repos": [
  >            {
  >              "repoid": 1,
  >              "default_action": "prepend_prefix",
  >              "default_prefix": "smallrepofolder1",
  >              "bookmark_prefix": "bookprefix1/",
  >              "mapping": {
  >                "special": "specialsmallrepofolder1"
  >              },
  >              "direction": "large_to_small"
  >            }
  >          ],
  >          "version_name": "TEST_VERSION_NAME_LIVE_V1"
  >        }
  >      ],
  >      "common": {
  >        "common_pushrebase_bookmarks": ["master_bookmark"],
  >        "large_repo_id": 0,
  >        "small_repos": {
  >          1: {
  >            "bookmark_prefix": "bookprefix1/"
  >          }
  >        }
  >      }
  >    }
  >  }
  > }
  > EOF

setup hg server repos
  $ function createfile { mkdir -p "$(dirname  $1)" && echo "$1" > "$1" && hg add -q "$1"; }
  $ function create_first_post_move_commit {
  >   echo 1 > "$1/filetoremove" && hg add "$1/filetoremove" && hg ci -m 'first post-move commit'
  >   hg revert -r .^ "$1/filetoremove"
  > }

  $ cd $TESTTMP
  $ hginit_treemanifest small-mon-1
  $ cd "$TESTTMP/small-mon-1"
  $ echo 1 > file.txt
  $ hg addremove -q && hg ci -q -m 'pre-move commit 1'

  $ cd "$TESTTMP"
  $ cp -r small-mon-1 large-mon
  $ cd large-mon
  $ mkdir smallrepofolder1
  $ hg mv file.txt smallrepofolder1/file.txt
  $ hg ci -m 'move commit'
  $ mkdir smallrepofolder2
  $ echo 2 > smallrepofolder2/file.txt
  $ hg addremove -q
  $ hg ci -m "move commit for repo 2"
  $ create_first_post_move_commit smallrepofolder1
  $ hg book -r . master_bookmark

  $ cd "$TESTTMP/small-mon-1"
  $ create_first_post_move_commit .
  $ hg book -r . master_bookmark

blobimport hg servers repos into Mononoke repos
  $ cd $TESTTMP
  $ REPOIDLARGE=0
  $ REPOIDSMALL1=1
  $ REPOID="$REPOIDLARGE" blobimport large-mon/.hg large-mon
  $ REPOID="$REPOIDSMALL1" blobimport small-mon-1/.hg small-mon-1

setup hg client repos
  $ function init_client() {
  > cd "$TESTTMP"
  > hg clone -q mono:"$1" "$2" --noupdate
  > cd "$TESTTMP/$2"
  > cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF
  > }

  $ init_client small-mon-1 small-hg-client-1
  $ cd "$TESTTMP"
  $ init_client large-mon large-hg-client

Setup helpers
  $ LARGE_MASTER_BONSAI=$(mononoke_newadmin bookmarks --repo-id $REPOIDLARGE get master_bookmark)
  $ SMALL1_MASTER_BONSAI=$(mononoke_newadmin bookmarks --repo-id $REPOIDSMALL1 get master_bookmark)

start mononoke server
  $ start_and_wait_for_mononoke_server
Make sure mapping is set up and we know what we don't have to sync initial entries
  $ add_synced_commit_mapping_entry $REPOIDSMALL1 $SMALL1_MASTER_BONSAI $REPOIDLARGE $LARGE_MASTER_BONSAI TEST_VERSION_NAME_LIVE_V1
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES ($REPOIDSMALL1, 'backsync_from_$REPOIDLARGE', 1)";

Normal pushrebase with one commit
  $ cd "$TESTTMP/small-hg-client-1"
  $ hg up -q master_bookmark
  $ echo 2 > 2 && hg addremove -q && hg ci -q -m newcommit
  $ hg push -r . --to master_bookmark 2>&1 | grep "updated remote bookmark"
  updated remote bookmark master_bookmark to 6989db12d1e5
-- newcommit was correctly pushed to master_bookmark
  $ log -r master_bookmark
  @  newcommit [public;rev=2;6989db12d1e5] default/master_bookmark
  │
  ~

-- newcommit is also present in the large repo (after a pull)
  $ cd "$TESTTMP"/large-hg-client
  $ log -r master_bookmark
  o  first post-move commit [public;rev=3;bca7e9574548] default/master_bookmark
  │
  ~
  $ hg pull -q
  $ log -r master_bookmark
  o  newcommit [public;rev=4;7c9a729ceb57] default/master_bookmark
  │
  ~

Live change of the config, without Mononoke restart
  $ update_commit_sync_map_second_option

-- let LiveCommitSyncConfig pick up the changes
  $ force_update_configerator

  $ cd "$TESTTMP"/small-hg-client-1
  $ hg up master_bookmark -q
  $ echo 1 >> 1 && hg add 1 && hg ci -m 'change of mapping'
  $ hg revert -r .^ 1
  $ hg commit --amend
  $ hg push -r . --to master_bookmark -q
  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $REPOIDLARGE --target-repo-id $REPOIDSMALL1 check-push-redirection-prereqs master_bookmark master_bookmark TEST_VERSION_NAME_LIVE_V1
  * all is well! (glob)
  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $REPOIDLARGE --target-repo-id $REPOIDSMALL1 check-push-redirection-prereqs master_bookmark master_bookmark TEST_VERSION_NAME_LIVE_V2
  * all is well! (glob)
  $ mononoke_admin_source_target $REPOIDLARGE $REPOIDSMALL1 crossrepo pushredirection change-mapping-version \
  > --author author \
  > --large-repo-bookmark master_bookmark \
  > --version-name TEST_VERSION_NAME_LIVE_V2 \
  > --via-extra &>/dev/null
  $ export REPOIDLARGE=0
  $ export REPOIDSMALL=1
  $ backsync_large_to_small 2>&1 | grep "force using"
  * force using mapping TEST_VERSION_NAME_LIVE_V2 to rewrite * (glob)
  $ flush_mononoke_bookmarks

-- wait until it backsyncs to a small repo
  $ sleep 2
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT small_repo_id, large_repo_id, sync_map_version_name, source_repo FROM synced_commit_mapping";
  1|0|TEST_VERSION_NAME_LIVE_V1|large
  1|0|TEST_VERSION_NAME_LIVE_V1|small
  1|0|TEST_VERSION_NAME_LIVE_V1|small
  1|0|TEST_VERSION_NAME_LIVE_V2|large

Do a push it should fail because we disallow pushing over a changeset that changes the mapping
  $ mkdir -p special
  $ echo f > special/f && hg ci -Aqm post_config_change_commit
  $ hg push -r . --to master_bookmark
  pushing rev 318b198c67b1 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (ef1b32df8f95, 318b198c67b1] (1 commit) to remote bookmark master_bookmark
  abort: Server error: invalid request: Pushrebase failed: Force failed pushrebase, please do a manual rebase. (Bonsai changeset id that triggered it is *) (glob)
  [255]

Again, normal pushrebase with one commit
  $ cd "$TESTTMP/small-hg-client-1"
  $ hg st
  $ hg pull -q
  $ hg up -q master_bookmark
  $ hg log -r master_bookmark
  commit:      * (glob)
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  user:        author
  date:        * (glob)
  summary:     Changing synced mapping version to TEST_VERSION_NAME_LIVE_V2 for large-mon->small-mon-1 sync
  
  $ mkdir -p special
  $ echo f > special/f && hg ci -Aqm post_config_change_commit
  $ hg push -r . --to master_bookmark 2>&1 | grep "updated remote bookmark"
  updated remote bookmark master_bookmark to * (glob)

-- in the large repo, new commit touched an after_change path
  $ cd "$TESTTMP"/large-hg-client
  $ hg pull -q
  $ hg log -T "{files % '{file}\n'}" -r master_bookmark
  specialsmallrepofolder_after_change/f

  $ EXPECTED_RC=1 quiet_grep "NonRootMPath" -- megarepo_tool_multirepo --source-repo-id $REPOIDLARGE --target-repo-id $REPOIDSMALL1 check-push-redirection-prereqs master_bookmark master_bookmark TEST_VERSION_NAME_LIVE_V1
  Some(NonRootMPath("special/f")) is a file in small-mon-1, but nonexistant in large-mon (under Some(NonRootMPath("specialsmallrepofolder1/f")))
  Some(NonRootMPath("special/f")) is a file in small-mon-1, but nonexistant in large-mon (under Some(NonRootMPath("specialsmallrepofolder1/f")))
  [1]
  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $REPOIDLARGE --target-repo-id $REPOIDSMALL1 check-push-redirection-prereqs master_bookmark master_bookmark TEST_VERSION_NAME_LIVE_V2
  * all is well! (glob)

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT small_repo_id, large_repo_id, sync_map_version_name, source_repo FROM synced_commit_mapping";
  1|0|TEST_VERSION_NAME_LIVE_V1|large
  1|0|TEST_VERSION_NAME_LIVE_V1|small
  1|0|TEST_VERSION_NAME_LIVE_V1|small
  1|0|TEST_VERSION_NAME_LIVE_V2|large
  1|0|TEST_VERSION_NAME_LIVE_V1|small
  1|0|TEST_VERSION_NAME_LIVE_V2|small
