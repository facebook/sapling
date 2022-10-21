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
  > name="master_bookmark"
  > [[bookmarks.hooks]]
  > hook_name="deny_files"
  > [[hooks]]
  > name="deny_files"
  > [hooks.config_string_lists]
  >   deny_patterns = [
  >     "/[.]git/",
  >   ]
  > CONFIG
  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

Normal pushrebase to the small repo with one commit succeeds, and does not get
blocked by deny_files
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ echo 2 > 2 && hg addremove -q && hg ci -q -m newcommit
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark
-- newcommit was correctly pushed to master_bookmark
  $ log -r master_bookmark
  @  newcommit [public;rev=2;ce81c7d38286] default/master_bookmark
  │
  ~

-- newcommit is also present in the large repo (after a pull)
  $ cd "$TESTTMP"/large-hg-client
  $ log -r master_bookmark
  @  first post-move commit [public;rev=2;bfcfb674663c] default/master_bookmark
  │
  ~
  $ REPONAME=large-mon hgmn pull -q
  $ log -r master_bookmark
  o  newcommit [public;rev=3;819e91b238b7] default/master_bookmark
  │
  ~
- compare the working copies
  $ verify_wc master_bookmark

Pushing to the small repo triggers deny_files, even though deny_files is only configured on the large repo
Note that the node is from the small repo, even though the hook is in the large repo

  $ cd "$TESTTMP"/small-hg-client
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ mkdir -p f/.git
  $ echo 2 > f/.git/HEAD && hg addremove -q && hg ci -q -m .git
  $ hg log -T"small_node: {node}\n" -r .
  small_node: 6e6a22d48eb51db1e7b8af685d9c99c0d7f10f70
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark
  pushing rev 6e6a22d48eb5 to destination mononoke://$LOCALIP:$LOCAL_PORT/small-mon bookmark master_bookmark
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

Let's check that disabling running pushredirected hooks work
  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
  >     "disable_running_hooks_in_pushredirected_repo": true
  >   }
  > }
  > EOF

  $ force_update_configerator
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark
  pushing rev 6e6a22d48eb5 to destination mononoke://$LOCALIP:$LOCAL_PORT/small-mon bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
