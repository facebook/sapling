# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ setup_configerator_configs

  $ setconfig push.edenapi=true
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
  $ enable_pushredirect 1
  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

Normal pushrebase to the small repo with one commit succeeds, and does not get
blocked by deny_files
  $ cd "$TESTTMP/small-hg-client"
  $ hg up -q master_bookmark
  $ echo 2 > 2 && hg addremove -q && hg ci -q -m newcommit
  $ hg push -r . --to master_bookmark 2>&1 | grep "updated remote bookmark"
  updated remote bookmark master_bookmark to ce81c7d38286
-- newcommit was correctly pushed to master_bookmark
  $ log -r master_bookmark
  @  newcommit [public;rev=2;ce81c7d38286] remote/master_bookmark
  │
  ~

-- newcommit is also present in the large repo (after a pull)
  $ cd "$TESTTMP"/large-hg-client
  $ log -r master_bookmark
  @  first post-move commit [public;rev=2;bfcfb674663c] remote/master_bookmark
  │
  ~
  $ hg pull -q
  $ log -r master_bookmark
  o  newcommit [public;rev=3;819e91b238b7] remote/master_bookmark
  │
  ~
- compare the working copies
  $ verify_wc $(hg log -r master_bookmark -T '{node}')

Pushing to the small repo triggers deny_files, even though deny_files is only configured on the large repo
Note that the node is from the small repo, even though the hook is in the large repo
To create a commit with `.git` in modified file path, use `debugdrawdag` to bypass the working copy path auditor.

  $ cd "$TESTTMP"/small-hg-client
  $ hg up -q master_bookmark
  $ hg debugdrawdag --no-bookmarks << 'EOS'
  > GIT  # GIT/f/.git/HEAD=2\n
  > |
  > master_bookmark
  > EOS
  $ hg log -T"small_node: {node}\n" -r 'desc(GIT)'
  small_node: fecac23a93122914ac16a11bca8e6c4c1b17314c
  $ hg push -r 'desc(GIT)' --to master_bookmark
  pushing rev fecac23a9312 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 3 trees for upload
  edenapi: uploaded 3 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (ce81c7d38286, fecac23a9312] (1 commit) to remote bookmark master_bookmark
  abort: Server error: hooks failed:
    deny_files for 8bd0a7cd107ee1da0f08efe9493d8ad68dcc7c2f6a3362e2ca71fd602518aa07: Denied filename 'smallrepofolder/f/.git/HEAD' matched name pattern '/[.]git/'. Rename or remove this file and try again.
  [255]

Let's check that disabling running pushredirected hooks work
  $ merge_just_knobs <<EOF
  > {
  >    "bools": {
  >      "scm/mononoke:disable_running_hooks_in_pushredirected_repo": true
  >    }
  > }
  > EOF

  $ force_update_configerator
  $ hg push -r 'desc(GIT)' --to master_bookmark
  pushing rev fecac23a9312 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  pushrebasing stack (ce81c7d38286, fecac23a9312] (1 commit) to remote bookmark master_bookmark
  updated remote bookmark master_bookmark to fecac23a9312
