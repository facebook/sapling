# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
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

  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
  >     "run_hooks_on_additional_changesets": true
  >   }
  > }
  > EOF

  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

We can't force pushrebase to a shared bookmark, so create a test bookmark that only belongs
to the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ REPONAME=small-mon hgmn push -r . --to test_bookmark --create
  pushing rev 11f848659bfc to destination mononoke://$LOCALIP:$LOCAL_PORT/small-mon bookmark test_bookmark
  searching for changes
  no changes found
  exporting bookmark test_bookmark

Force pushrebase to the small repo with one commit succeeds, and does not get
blocked by deny_files
  $ echo 2 > 2 && hg addremove -q && hg ci -q -m newcommit
  $ REPONAME=small-mon hgmn push -r . --to test_bookmark --force 2>&1 | grep updating
  updating bookmark test_bookmark
-- newcommit was correctly pushed to test_bookmark
  $ log -r test_bookmark
  @  newcommit [public;rev=2;ce81c7d38286] default/test_bookmark
  │
  ~

-- newcommit is also present in the large repo (after a pull)
  $ cd "$TESTTMP"/large-hg-client
  $ REPONAME=large-mon hgmn pull -q
  $ log -r bookprefix/test_bookmark
  o  newcommit [public;rev=3;819e91b238b7] default/bookprefix/test_bookmark
  │
  ~
- compare the working copies
  $ verify_wc bookprefix/test_bookmark

Pushing to the small repo triggers deny_files, even though deny_files is only configured on the large repo.
Note that the node is from the small repo, even though the hook is in the large repo

  $ cd "$TESTTMP"/small-hg-client
  $ REPONAME=small-mon hgmn up -q test_bookmark
  $ mkdir -p f/.git
  $ echo 2 > f/.git/HEAD && hg addremove -q && hg ci -q -m .git
  $ hg log -T"small_node: {node}\n" -r .
  small_node: 6e6a22d48eb51db1e7b8af685d9c99c0d7f10f70
  $ REPONAME=small-mon hgmn push -r . --to test_bookmark --force
  pushing rev 6e6a22d48eb5 to destination mononoke://$LOCALIP:$LOCAL_PORT/small-mon bookmark test_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     deny_files for 6e6a22d48eb51db1e7b8af685d9c99c0d7f10f70: Denied filename 'smallrepofolder/f/.git/HEAD' matched name pattern '/[.]git/'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     deny_files for 6e6a22d48eb51db1e7b8af685d9c99c0d7f10f70: Denied filename 'smallrepofolder/f/.git/HEAD' matched name pattern '/[.]git/'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\ndeny_files for 6e6a22d48eb51db1e7b8af685d9c99c0d7f10f70: Denied filename 'smallrepofolder/f/.git/HEAD' matched name pattern '/[.]git/'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]

Create a commit in the large repo that triggers deny_files.  Since we haven't enabled the hook
there, we are ok to create it.  Create a commit on top of that that is backsynced.

  $ cd "$TESTTMP"/large-hg-client
  $ REPONAME=large-mon hgmn up -q master_bookmark
  $ mkdir -p x/.git
  $ echo 2 > x/.git/HEAD && hg addremove -q && hg ci -q -m .git-large
  $ hg log -T "large_node: {node}\n" -r .
  large_node: d967862de4d54c47ba51e0259fb1f72d881efd73
  $ echo 3 > smallrepofolder/largerepofile && hg addremove -q && hg ci -q -m backsync
  $ REPONAME=large-mon hgmn push --to master_bookmark
  pushing rev 148264a57519 to destination mononoke://$LOCALIP:$LOCAL_PORT/large-mon bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
  $ backsync_large_to_small 2>&1 | grep "syncing bookmark"
  * syncing bookmark master_bookmark to * (glob)
  $ flush_mononoke_bookmarks

Commit has been backsynced
  $ cd "$TESTTMP"/small-hg-client
  $ REPONAME=small-mon hgmn pull -q
  $ log -r master_bookmark
  o  backsync [public;rev=4;cd9bfa9f25eb] default/master_bookmark
  │
  ~

Attempt to move test_bookmark to the new master_bookmark commit.  It fails because of the
hook in the large repo.
Note that since the large repo commit doesn't map to the small repo, we see the large repo
changeset id.

  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ REPONAME=small-mon hgmn push -r . --to test_bookmark --pushvar NON_FAST_FORWARD=true
  pushing rev cd9bfa9f25eb to destination mononoke://$LOCALIP:$LOCAL_PORT/small-mon bookmark test_bookmark
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     deny_files for d967862de4d54c47ba51e0259fb1f72d881efd73: Denied filename 'x/.git/HEAD' matched name pattern '/[.]git/'. Rename or remove this file and try again.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     deny_files for d967862de4d54c47ba51e0259fb1f72d881efd73: Denied filename 'x/.git/HEAD' matched name pattern '/[.]git/'. Rename or remove this file and try again.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\ndeny_files for d967862de4d54c47ba51e0259fb1f72d881efd73: Denied filename 'x/.git/HEAD' matched name pattern '/[.]git/'. Rename or remove this file and try again."
  abort: unexpected EOL, expected netstring digit
  [255]
