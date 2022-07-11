# Copyright (c) Meta Platforms, Inc. and affiliates.
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
  $ PUSHREBASE_REWRITE_DATES=1 init_large_small_repo
  Setting up hg server repos
  Blobimporting them
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
  $ hg ci -Aqm "before config change"
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q
  $ log -r master_bookmark
  o  before config change [public;rev=4;*] default/master_bookmark (glob)
  │
  ~

-- wait a second to give backsyncer some time to catch up
  $ sleep 50
  $ flush_mononoke_bookmarks

-- check the same commit in the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ log -r master_bookmark
  @  before config change [public;rev=2;*] default/master_bookmark (glob)
  │
  ~
  $ hg log -r master_bookmark -T "{files % '{file}\n'}"
  bla

Config change
  $ update_commit_sync_map_first_option
-- let LiveCommitSyncConfig pick up the changes
  $ force_update_configerator

  $ cd "$TESTTMP"/large-hg-client
  $ REPONAME=large-mon hgmn up master_bookmark -q
  $ echo 1 >> 1 && hg add 1 && hg ci -m 'change of mapping'
  $ hg revert -r .^ 1
  $ hg commit --amend
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q

-- wait a second to give backsyncer some time to catch up
  $ sleep 50
  $ flush_mononoke_bookmarks
  $ LARGE_MASTER_BONSAI=$(get_bonsai_bookmark $REPOIDLARGE master_bookmark)
  $ SMALL_MASTER_BONSAI=$(get_bonsai_bookmark $REPOIDSMALL master_bookmark)
  $ update_mapping_version "$REPOIDSMALL" "$SMALL_MASTER_BONSAI" "$REPOIDLARGE" "$LARGE_MASTER_BONSAI" "new_version"

-- push to a large repo, using new path mapping
  $ cd "$TESTTMP"/large-hg-client
  $ REPONAME=large-mon hgmn up -q master_bookmark

  $ mkdir -p smallrepofolder_after
  $ echo baz > smallrepofolder_after/baz
  $ hg ci -Aqm "after config change"
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q
  $ log -r master_bookmark
  o  after config change [public;rev=*;*] default/master_bookmark (glob)
  │
  ~

-- wait a second to give backsyncer some time to catch up
  $ sleep 50
  $ flush_mononoke_bookmarks

-- check the same commit in the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ log -r master_bookmark
  @  after config change [public;rev=*;*] default/master_bookmark (glob)
  │
  ~
  $ hg log -r master_bookmark -T "{files % '{file}\n'}"
  baz
