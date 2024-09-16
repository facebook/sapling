# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 create_large_small_repo
  Adding synced mapping entry
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/large-mon/server.toml << CONFIG
  > [[bookmarks]]
  > name="bookprefix/test_bookmark"
  > [[bookmarks.hooks]]
  > hook_name="deny_files"
  > [[hooks]]
  > name="deny_files"
  > [hooks.config_string_lists]
  >   deny_patterns = [
  >     "/[.]git/",
  >   ]
  > CONFIG

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:run_hooks_on_additional_changesets": true
  >   }
  > }
  > EOF

  $ setup_configerator_configs
  $ enable_pushredirect 1

  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

We can't force pushrebase to a shared bookmark, so create a test bookmark that only belongs
to the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ hg up -q master_bookmark
  $ hg push -r . --to test_bookmark --create
  pushing rev 11f848659bfc to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark test_bookmark
  creating remote bookmark test_bookmark

Force pushrebase to the small repo with one commit succeeds, and does not get
blocked by deny_files
  $ echo 2 > 2 && hg addremove -q && hg ci -q -m newcommit
  $ hg push -r . --to test_bookmark --force 2>&1 | grep moving
  moving remote bookmark test_bookmark from 11f848659bfc to ce81c7d38286
-- newcommit was correctly pushed to test_bookmark
  $ log -r test_bookmark
  @  newcommit [public;rev=2;ce81c7d38286] default/test_bookmark
  │
  ~

-- newcommit is also present in the large repo (after a pull)
  $ cd "$TESTTMP"/large-hg-client
  $ hg pull -q
  $ log -r bookprefix/test_bookmark
  o  newcommit [public;rev=3;819e91b238b7] default/bookprefix/test_bookmark
  │
  ~
- compare the working copies
  $ verify_wc $(hg log -r bookprefix/test_bookmark -T '{node}')

Pushing to the small repo triggers deny_files, even though deny_files is only configured on the large repo.
Note that the node is from the small repo, even though the hook is in the large repo

  $ cd "$TESTTMP"/small-hg-client
  $ hg up -q test_bookmark
  $ mkdir -p f/.git
  $ echo 2 > f/.git/HEAD && hg addremove -q && hg ci -q -m .git
  $ hg log -T"small_node: {node}\n" -r .
  small_node: 6e6a22d48eb51db1e7b8af685d9c99c0d7f10f70
  $ hg push -r . --to test_bookmark --force
  pushing rev 6e6a22d48eb5 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark test_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 3 trees for upload
  edenapi: uploaded 3 trees
  edenapi: uploaded 1 changeset
  moving remote bookmark test_bookmark from ce81c7d38286 to 6e6a22d48eb5
  abort: server error: hooks failed:
    deny_files for b5ac9b3203d4aef816083f98fd6f169d701c6ae41d08e49d9abc6b0ae5318bbe: Denied filename 'smallrepofolder/f/.git/HEAD' matched name pattern '/[.]git/'. Rename or remove this file and try again.
  [255]

Create a commit in the large repo that triggers deny_files.  Since we haven't enabled the hook
there, we are ok to create it.  Create a commit on top of that that is backsynced.

  $ cd "$TESTTMP"/large-hg-client
  $ hg up -q master_bookmark
  $ mkdir -p x/.git
  $ echo 2 > x/.git/HEAD && hg addremove -q && hg ci -q -m .git-large
  $ hg log -T "large_node: {node}\n" -r .
  large_node: d967862de4d54c47ba51e0259fb1f72d881efd73
  $ echo 3 > smallrepofolder/largerepofile && hg addremove -q && hg ci -q -m backsync
  $ hg push --to master_bookmark
  pushing rev 148264a57519 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 5 trees for upload
  edenapi: uploaded 5 trees
  edenapi: uploaded 2 changesets
  pushrebasing stack (bfcfb674663c, 148264a57519] (2 commits) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 148264a57519
  $ backsync_large_to_small 2>&1 | grep "syncing bookmark"
  * syncing bookmark master_bookmark to * (glob)
  $ flush_mononoke_bookmarks

Commit has been backsynced
  $ cd "$TESTTMP"/small-hg-client
  $ hg pull -q
  $ log -r master_bookmark
  o  backsync [public;rev=4;cd9bfa9f25eb] default/master_bookmark
  │
  ~

Attempt to move test_bookmark to the new master_bookmark commit.
No hook runs because the hooks already ran for this changeset.

  $ hg up -q master_bookmark
  $ hg push -r . --to test_bookmark --pushvar NON_FAST_FORWARD=true
  pushing rev cd9bfa9f25eb to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark test_bookmark
  moving remote bookmark test_bookmark from ce81c7d38286 to cd9bfa9f25eb
