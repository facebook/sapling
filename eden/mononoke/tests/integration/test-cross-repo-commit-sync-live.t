# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

Setup configuration
  $ setup_configerator_configs
  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "1": {
  >      "draft_push": false,
  >      "public_push": false
  >    }
  >   }
  > }
  > EOF

-- Init Mononoke thingies
  $ XREPOSYNC=1 init_large_small_repo --local-configerator-path="$TESTTMP/configerator"
  Setting up hg server repos
  Blobimporting them
  Adding synced mapping entry
  Starting Mononoke server

-- Start up the sync job in the background
  $ mononoke_x_repo_sync_forever $REPOIDSMALL $REPOIDLARGE --local-configerator-path="$TESTTMP/configerator"

Before the change
-- push to a small repo
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ mkdir -p non_path_shifting
  $ echo a > foo
  $ echo b > non_path_shifting/bar
  $ hg ci -Aqm "before config change"
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark -q
  $ log -r master_bookmark
  @  before config change [public;rev=2;bc6a206054d0] default/master_bookmark
  |
  ~

-- wait a little to give sync job some time to catch up
  $ sleep 3

-- check the same commit in the large repo
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ REPONAME=large-mon hgmn up -q master_bookmark
  $ log -r master_bookmark
  @  before config change [public;rev=3;c76f6510b5c1] default/master_bookmark
  |
  ~
  $ REPONAME=large-mon hgmn log -r master_bookmark -T "{files % '{file}\n'}"
  non_path_shifting/bar
  smallrepofolder/foo

Make a config change
  $ cat > "$COMMIT_SYNC_CONF/current" << EOF
  > {
  >   "repos": {
  >     "megarepo_test": {
  >       "large_repo_id": 0,
  >       "common_pushrebase_bookmarks": ["master_bookmark"],
  >       "small_repos": [
  >         {
  >           "repoid": 1,
  >           "bookmark_prefix": "bookprefix/",
  >           "default_action": "prepend_prefix",
  >           "default_prefix": "smallrepofolder_after",
  >           "direction": "large_to_small",
  >           "mapping": {
  >             "non_path_shifting": "non_path_shifting"
  >           }
  >         }
  >       ]
  >     }
  >   }
  > }
  > EOF
-- let LiveCommitSyncConfig pick up the changes
  $ sleep 2

After the change
-- push to a small repo
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ echo a > boo
  $ echo b > non_path_shifting/baz
  $ hg ci -Aqm "after config change"
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark -q
  $ log -r master_bookmark
  @  after config change [public;rev=3;6b8e5fe49ff9] default/master_bookmark
  |
  ~

-- wait a little to give sync job some time to catch up
  $ sleep 3

-- check the same commit in the large repo
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ REPONAME=large-mon hgmn up -q master_bookmark
  $ log -r master_bookmark
  @  after config change [public;rev=4;f73b39d6fa97] default/master_bookmark
  |
  ~
  $ REPONAME=large-mon hgmn log -r master_bookmark -T "{files % '{file}\n'}"
  non_path_shifting/baz
  smallrepofolder_after/boo
