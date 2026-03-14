# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ setconfig push.edenapi=true
  $ create_large_small_repo
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

  $ setup_configerator_configs
  $ enable_pushredirect 1

  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

  $ setconfig remotenames.selectivepulldefault=master_bookmark,bookprefix/test_bookmark,test_bookmark

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
  @  newcommit [public;rev=281474976710656;ce81c7d38286] remote/test_bookmark
  │
  ~

-- newcommit is also present in the large repo (after a pull)
  $ cd "$TESTTMP"/large-hg-client
  $ hg pull -q
  $ log -r bookprefix/test_bookmark
  o  newcommit [public;rev=281474976710656;819e91b238b7] remote/bookprefix/test_bookmark
  │
  ~
- compare the working copies
  $ verify_wc $(hg log -r bookprefix/test_bookmark -T '{node}')

Pushing to the small repo triggers deny_files, even though deny_files is only configured on the large repo.
Note that the node is from the small repo, even though the hook is in the large repo
(use testtool_drawdag since Sapling client rejects .git paths)

  $ cd "$TESTTMP"/small-hg-client
  $ testtool_drawdag -R small-mon --print-hg-hashes <<EOF
  > B
  > |
  > A
  > # exists: A $SMALL_MASTER_BONSAI
  > # modify: B "f/.git/HEAD" "2\n"
  > # message: B ".git"
  > # author: B test
  > EOF
  A=11f848659bfcf77abd04f947883badd8efa88d26
  B=94b4d63cb3185098ded56065f2f9f3d9e61cf1fe

  $ hg pull -q -r $B
  $ hg log -T"small_node: {node}\n" -r $B
  small_node: 94b4d63cb3185098ded56065f2f9f3d9e61cf1fe
  $ hg push -r $B --to test_bookmark --force
  pushing rev 94b4d63cb318 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark test_bookmark
  moving remote bookmark test_bookmark from ce81c7d38286 to 94b4d63cb318
  abort: server error: hooks failed:
    deny_files for 45dacb440475146894aee9056c136bb72d64454729080e71d46b4dbee3afd233: Denied filename 'smallrepofolder/f/.git/HEAD' matched name pattern '/[.]git/'. Rename or remove this file and try again.
  [255]

Create a commit in the large repo that triggers deny_files.  Since we haven't enabled the hook
there, we are ok to create it.  Create a commit on top of that that is backsynced.
(use testtool_drawdag since Sapling client rejects .git paths)

  $ testtool_drawdag -R large-mon --print-hg-hashes <<EOF
  > E
  > |
  > D
  > |
  > C
  > # exists: C $LARGE_MASTER_BONSAI
  > # modify: D "x/.git/HEAD" "2\n"
  > # message: D ".git-large"
  > # author: D test
  > # modify: E "smallrepofolder/largerepofile" "3\n"
  > # message: E "backsync"
  > # author: E test
  > # bookmark: E master_bookmark
  > EOF
  C=bfcfb674663c5438027bcde4a7ae5024c838f76a
  D=8fd531f07276538a04d156d382db5b30611cdb4c
  E=9af5392c08a0812660278cbb0242e2171327c6a3

  $ cd "$TESTTMP"/large-hg-client
  $ hg pull -q
  $ hg log -T "large_node: {node}\n" -r $D
  large_node: 8fd531f07276538a04d156d382db5b30611cdb4c
  $ backsync_large_to_small 2>&1 | grep "syncing bookmark"
  * syncing bookmark master_bookmark to * (glob)
  $ flush_mononoke_bookmarks

Commit has been backsynced
  $ cd "$TESTTMP"/small-hg-client
  $ hg pull -q
  $ log -r master_bookmark
  o  backsync [public;rev=2;37a9a8a030b3] remote/master_bookmark
  │
  ~

Attempt to move test_bookmark to the new master_bookmark commit.
No hook runs because the hooks already ran for this changeset.

  $ hg up -q master_bookmark
  $ hg push -r . --to test_bookmark --pushvar NON_FAST_FORWARD=true
  pushing rev 37a9a8a030b3 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark test_bookmark
  moving remote bookmark test_bookmark from ce81c7d38286 to 37a9a8a030b3
