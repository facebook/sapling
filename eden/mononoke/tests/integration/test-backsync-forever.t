# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ setup_configerator_configs
  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "1": {
  >      "draft_push": false,
  >      "public_push": true
  >    }
  >   }
  > }
  > EOF

-- Init Mononoke thingies
  $ PUSHREBASE_REWRITE_DATES=1 init_large_small_repo --local-configerator-path="$TESTTMP/configerator"
  Setting up hg server repos
  Blobimporting them
  Adding synced mapping entry
  Starting Mononoke server

-- Start up the backsyncer in the background
  $ backsync_large_to_small_forever --local-configerator-path="$TESTTMP/configerator"

Before config change
-- push to a large repo
  $ cd "$TESTTMP"/large-hg-client
  $ REPONAME=large-mon hgmn up -q master_bookmark

  $ mkdir -p smallrepofolder
  $ echo bla > smallrepofolder/bla
  $ hg ci -Aqm "before config change"
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q
  $ log -r master_bookmark
  o  before config change [public;rev=4;*] default/master_bookmark (glob)
  |
  ~

-- wait a second to give backsyncer some time to catch up
  $ sleep 3

-- check the same commit in the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ log -r master_bookmark
  @  before config change [public;rev=2;*] default/master_bookmark (glob)
  |
  ~
  $ hg log -r master_bookmark -T "{files % '{file}\n'}"
  bla

Config change
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

Backsync after the change
-- push to a large repo, using new path mapping
  $ cd "$TESTTMP"/large-hg-client
  $ REPONAME=large-mon hgmn up -q master_bookmark

  $ mkdir -p smallrepofolder_after
  $ echo baz > smallrepofolder_after/baz
  $ hg ci -Aqm "after config change"
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q
  $ log -r master_bookmark
  o  after config change [public;rev=6;*] default/master_bookmark (glob)
  |
  ~

-- wait a second to give backsyncer some time to catch up
  $ sleep 3

-- check the same commit in the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ log -r master_bookmark
  @  after config change [public;rev=3;*] default/master_bookmark (glob)
  |
  ~
  $ hg log -r master_bookmark -T "{files % '{file}\n'}"
  baz
