# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ enable remotenames

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

  $ PUSHREBASE_REWRITE_DATES=1 init_large_small_repo
  Setting up hg server repos
  Blobimporting them
  Adding synced mapping entry
  Starting Mononoke server

-- enable verification hook in small-hg-srv
  $ cd "$TESTTMP/small-hg-srv"
  $ enable_replay_verification_hook

-- normal pushrebase with one commit
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ echo 2 > 2 && hg addremove -q && hg ci -q -m newcommit
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark
-- newcommit was correctly pushed to master_bookmark (we need to update, as it's a new commit with date rewriting)
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ log -r master_bookmark
  @  newcommit [public;rev=3;*] default/master_bookmark (glob)
  │
  ~
-- newcommit is also present in the large repo (after a pull)
  $ cd "$TESTTMP"/large-hg-client
  $ log -r master_bookmark
  o  first post-move commit [public;rev=2;*] default/master_bookmark (glob)
  │
  ~
  $ REPONAME=large-mon hgmn pull -q
  $ log -r master_bookmark
  o  newcommit [public;rev=3;*] default/master_bookmark (glob)
  │
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

  $ cd "$TESTTMP"
  $ REPOID="$REPOIDSMALL" mononoke_hg_sync small-hg-srv 2 2>&1 | grep "successful sync"
  * successful sync of entries [4]* (glob)

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from bundle_replay_data where bookmark_update_log_id = 6"
  $ REPOID="$REPOIDSMALL" mononoke_hg_sync small-hg-srv 5 2>&1 | grep "successful sync"
  * successful sync of entries [6]* (glob)
  $ cd small-hg-srv
  $ log -r :
  o  empty [draft;rev=3;*] (glob)
  │
  o  newcommit [draft;rev=2;*] (glob)
  │
  @  first post-move commit [draft;rev=1;*] (glob)
  │
  o  pre-move commit [draft;rev=0;*] (glob)
  $

  $ hg show master_bookmark
  commit:      * (glob)
  bookmark:    master_bookmark
  user:        test
  date:        * (glob)
  description:
  empty
  
  
  

Check that admin-created bookmark sets and deletes in the large repo can be correctly synced
-- Let's cover creation, updating and deletion of the bookmark
  $ cd "$TESTTMP/large-hg-client"
  $ REPOID="$REPOIDLARGE" mononoke_admin bookmarks set bookprefix/foobar $(hg log -T "{node}" -r master_bookmark) &>/dev/null
  $ backsync_large_to_small 2>&1 | grep creating
  * creating bookmark BookmarkName { bookmark: "foobar" } * (glob)
  $ REPOID="$REPOIDLARGE" mononoke_admin bookmarks set bookprefix/foobar $(hg log -T "{node}" -r master_bookmark~1) &>/dev/null
  $ backsync_large_to_small 2>&1 2>&1 | grep updating
  * updating bookmark BookmarkName { bookmark: "foobar" } * (glob)
  $ REPOID="$REPOIDLARGE" mononoke_admin bookmarks delete bookprefix/foobar &>/dev/null
  $ backsync_large_to_small 2>&1 | grep deleting
  * deleting bookmark BookmarkName { bookmark: "foobar" } * (glob)
