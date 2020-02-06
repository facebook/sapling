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

  $ PUSHREBASE_REWRITE_DATES=1 init_large_small_repo --local-configerator-path="$TESTTMP/configerator"
  Setting up hg server repos
  Blobimporting them
  Starting Mononoke server
  Adding synced mapping entry

-- enable verification hook in small-hg-srv
  $ cd "$TESTTMP/small-hg-srv"
  $ enable_replay_verification_hook

-- normal pushrebase with one commit
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ echo 2 > 2 && hg addremove -q && hg ci -q -m newcommit
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark | grep updating
  updating bookmark master_bookmark
-- newcommit was correctly pushed to master_bookmark (we need to update, as it's a new commit with date rewriting)
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ log -r master_bookmark
  @  newcommit [public;rev=3;*] default/master_bookmark (glob)
  |
  ~
-- newcommit is also present in the large repo (after a pull)
  $ cd "$TESTTMP"/large-hg-client
  $ log -r master_bookmark
  o  first post-move commit [public;rev=2;*] default/master_bookmark (glob)
  |
  ~
  $ REPONAME=large-mon hgmn pull -q
  $ log -r master_bookmark
  o  newcommit [public;rev=3;*] default/master_bookmark (glob)
  |
  ~
  $ verify_wc master_bookmark
-- do a push to a large repo, then backsync it to a small one
  $ REPONAME=large-mon hgmn up -q master_bookmark
  $ echo test > tolarge
  $ hg add tolarge
  $ hg ci -m tolarge
  $ echo 1 > empty && hg add empty && hg ci -m empty
  $ hg revert -r .^ empty
  $ hg commit --amend
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q
  $ backsync_large_to_small 2>&1 | grep "syncing bookmark"
  * syncing bookmark master_bookmark to * (glob)

-- mononoke hg sync job: the commit is now present in the small hg repo server
  $ cd "$TESTTMP"
  $ REPOID="$REPOIDSMALL" mononoke_hg_sync small-hg-srv 2 --use-existing-bundle-if-available 2>&1 | grep "successful sync"
  * successful sync of entries [4] (glob)

-- mononoke hg sync job: do a second sync, but this time without the bundle
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from bundle_replay_data where bookmark_update_log_id = 6"
  $ REPOID="$REPOIDSMALL" mononoke_hg_sync small-hg-srv 5 --use-existing-bundle-if-available 2>&1 | grep "successful sync"
  * successful sync of entries [6] (glob)
  $ cd small-hg-srv
  $ log -r :
  o  empty [public;rev=3;*] (glob)
  |
  o  newcommit [public;rev=2;*] (glob)
  |
  @  first post-move commit [public;rev=1;*] (glob)
  |
  o  pre-move commit [public;rev=0;*] (glob)
  $

  $ hg show master_bookmark
  changeset:   * (glob)
  bookmark:    master_bookmark
  user:        test
  date:        * (glob)
  description:
  empty
  
  
  
