# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ enable remotenames

  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 PUSHREBASE_REWRITE_DATES=1 create_large_small_repo
  Adding synced mapping entry
  $ setup_configerator_configs
  $ enable_pushredirect 1
  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

-- enable verification hook in small-hg-srv
  $ hginit_treemanifest "$TESTTMP/small-hg-srv"
  $ cd "$TESTTMP/small-hg-srv"
  $ enable_replay_verification_hook

-- normal pushrebase with one commit
  $ cd "$TESTTMP/small-hg-client"
  $ hg up -q master_bookmark
  $ echo 2 > 2 && hg addremove -q && hg ci -q -m newcommit
  $ hg push -r . --to master_bookmark 2>&1 | grep "updated remote bookmark"
  updated remote bookmark master_bookmark to * (glob)
-- newcommit was correctly pushed to master_bookmark (we need to update, as it's a new commit with date rewriting)
  $ hg up -q master_bookmark
  $ log -r master_bookmark
  @  newcommit [public;rev=3;*] default/master_bookmark (glob)
  │
  ~
-- newcommit is also present in the large repo (after a pull)
  $ cd "$TESTTMP"/large-hg-client
  $ log -r master_bookmark
  @  first post-move commit [public;rev=2;*] default/master_bookmark (glob)
  │
  ~
  $ hg pull -q
  $ log -r master_bookmark
  o  newcommit [public;rev=3;*] default/master_bookmark (glob)
  │
  ~
  $ verify_wc $(hg log -r master_bookmark -T '{node}')
-- do a push to a large repo, then backsync it to a small one
  $ hg up -q master_bookmark
  $ echo test > tolarge
  $ hg add tolarge
  $ hg ci -m tolarge
  $ echo 1 > empty && hg add empty && hg ci -m empty
  $ hg revert -r .^ empty
  $ hg commit --amend
  $ hg push -r . --to master_bookmark -q
  $ backsync_large_to_small 2>&1 | grep "syncing bookmark"
  * syncing bookmark master_bookmark to * (glob)

  $ cd "$TESTTMP"
  $ REPOID="$REPOIDSMALL" mononoke_hg_sync small-hg-srv 0 2>&1 | grep "successful sync"
  * successful sync of entries [1]* (glob)

  $ REPOID="$REPOIDSMALL" mononoke_hg_sync small-hg-srv 1 2>&1 | grep "successful sync"
  * successful sync of entries [2]* (glob)

  $ REPOID="$REPOIDSMALL" mononoke_hg_sync small-hg-srv 2 2>&1 | grep "successful sync"
  * successful sync of entries [3]* (glob)
  $ cd small-hg-srv
  $ log -r :
  o  empty [draft;rev=3;*] (glob)
  │
  o  newcommit [draft;rev=2;*] (glob)
  │
  o  first post-move commit [draft;rev=1;*] (glob)
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
  $ mononoke_newadmin bookmarks --repo-id="$REPOIDLARGE" set bookprefix/foobar $(hg log -T "{node}" -r master_bookmark)
  Creating publishing bookmark bookprefix/foobar at * (glob)
  $ backsync_large_to_small 2>&1 | grep creating
  * creating bookmark * "foobar" * (glob)
  $ mononoke_newadmin bookmarks --repo-id="$REPOIDLARGE" set bookprefix/foobar $(hg log -T "{node}" -r master_bookmark~1)
  Updating publishing bookmark bookprefix/foobar from * to * (glob)
  $ backsync_large_to_small 2>&1 2>&1 | grep updating
  * updating bookmark * "foobar" * (glob)
  $ mononoke_newadmin bookmarks --repo-id="$REPOIDLARGE" delete bookprefix/foobar
  Deleting publishing bookmark bookprefix/foobar at * (glob)
  $ backsync_large_to_small 2>&1 | grep deleting
  * deleting bookmark * "foobar" * (glob)
