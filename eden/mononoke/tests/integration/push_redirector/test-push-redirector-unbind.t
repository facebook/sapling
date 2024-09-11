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
  $ setup_configerator_configs
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:sql_disable_auto_cache": true
  >   }
  > }
  > EOF
  $ enable_pushredirect 1
  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

-- normal pushrebase with one commit
  $ cd "$TESTTMP/small-hg-client"
  $ hg up -q master_bookmark
  $ echo 2 > 2 && hg addremove -q && hg ci -q -m newcommit
  $ hg push -r . --to master_bookmark 2>&1 | grep 'updated remote bookmark'
  updated remote bookmark master_bookmark to ce81c7d38286
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
  $ hg pull -q
  $ log -r master_bookmark
  o  newcommit [public;rev=3;819e91b238b7] default/master_bookmark
  │
  ~


-- unbind repositories and wait until it propagates
  $ enable_pushredirect 1 false false
  $ force_update_configerator

-- do a push from small repo, make sure it is not pushredirected to large repo
  $ cd "$TESTTMP/small-hg-client"
  $ echo 2_unbound > 2 && hg ci -q -m unbound_commit
  $ echo 3 > 3 && hg addremove 3 && hg ci -m 'first unbound commit'
  $ echo 4 > 4 && hg addremove 4 && hg ci -m 'second unbound commit'
  $ SMALL_NODE="$(hg log -r tip -T '{node}')"
  $ hg push -r . --to master_bookmark 2>&1 | grep 'updated remote bookmark'
  updated remote bookmark master_bookmark to 2c39dde79608
  $ cd "$TESTTMP"/large-hg-client
  $ hg pull -q &> /dev/null
  $ log -r master_bookmark
  o  newcommit [public;rev=3;819e91b238b7] default/master_bookmark
  │
  ~
  $ hg st --change master_bookmark
  A smallrepofolder/2

-- do a push from large repo as well
  $ cd "$TESTTMP/large-hg-client"
  $ hg up master_bookmark -q
  $ echo 'largerepocontent' > smallrepofolder/2
  $ hg ci -m 'large repo unbound commit'
  $ hg push -r . --to master_bookmark 2>&1 | grep 'updated remote bookmark'
  updated remote bookmark master_bookmark to c4fabb2e572b
  $ log -r master_bookmark
  @  large repo unbound commit [public;rev=4;c4fabb2e572b] default/master_bookmark
  │
  ~

-- now re-binding.
-- (might be wise to lock repos first in real scenario)
-- Step 1. large repo unbound commits need to be marked as not sync candidate, since they
-- should not ever be synced to a small repo.
  $ echo "$(hg log -r master_bookmark -T '{node}')" > "$TESTTMP/not-sync-candidates"
  $ megarepo_tool_multirepo --source-repo-id 1 --target-repo-id 0 mark-not-synced --input-file "$TESTTMP/not-sync-candidates" test_version 2> /dev/null

-- Step 2. then we need to sync new small repo commits to a large repo
  $ megarepo_tool_multirepo --source-repo-id 1 --target-repo-id 0 sync-commit-and-ancestors --commit-hash "$SMALL_NODE" 2>&1 | grep remapped
  * remapped to RewrittenAs(ChangesetId(Blake2(146b951933c6d1554a377d733af183659f61794da5c6537c5de68e52acd5e949)), CommitSyncConfigVersion("test_version")) (glob)
  $ HG_CS_ID="$(mononoke_newadmin convert --repo-id 0 --from bonsai --to hg --derive 146b951933c6d1554a377d733af183659f61794da5c6537c5de68e52acd5e949)"
  $ cd "$TESTTMP/large-hg-client"
  $ hg pull -r "$HG_CS_ID"
  pulling from mono:large-mon
  searching for changes
  adding changesets
  adding manifests
  adding file changes

-- Step 3. now do merge in the large repo that fixed working copy and push it
  $ hg up master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
-- note - --tool ':local' is used only in tests,
-- you need something smarter in prod!
  $ hg merge "$HG_CS_ID" --tool ':local'
  2 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'rebinding'
  $ hg push -r . --to master_bookmark -q
  $ LARGE_REBINDING="$(hg log -r master_bookmark -T '{node}')"

-- Step 4. create a commit that fixes working copy in the small repo and push it
  $ cd "$TESTTMP/small-hg-client"
  $ echo 'largerepocontent' > 2
  $ hg ci -qm 'rebinding'
  $ hg push -r . --to master_bookmark -q
  $ SMALL_REBINDING="$(hg log -r master_bookmark -T '{node}')"

-- Step 5. mark commits that fix working copy as rewritten
  $ megarepo_tool_multirepo --source-repo-id 1 --target-repo-id 0 check-push-redirection-prereqs "$SMALL_REBINDING" "$LARGE_REBINDING" test_version 2>&1 | grep 'all is well!'
  * all is well! (glob)
  $ mononoke_admin_source_target 0 1 crossrepo insert rewritten \
  > --source-hash "$LARGE_REBINDING" --target-hash "$SMALL_REBINDING" --version-name test_version 2>&1 | grep 'successfully inserted'
  * successfully inserted rewritten mapping entry (glob)

-- Step 6. Rebind repositories and wait until it propagates
  $ mononoke_newadmin cross-repo --source-repo-name large-mon --target-repo-name small-mon pushredirection prepare-rollout &> /dev/null
  $ enable_pushredirect 1
  $ force_update_configerator

-- Verify it works fine
-- Do a new push from small repo from one of the
-- unbound commits
  $ cd "$TESTTMP/small-hg-client"
  $ hg up -q "$SMALL_NODE"
  $ echo 'newfile' > newfile
  $ hg add newfile
  $ hg ci -qm 'after rebinding'
  $ hg push -r . --to master_bookmark -q
  $ hg log -r master_bookmark
  commit:      ad40a9a26fbd
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     after rebinding
  
  $ cd "$TESTTMP/large-hg-client"
  $ hg pull -q
  $ hg log -r master_bookmark
  commit:      57b52edb15eb
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     after rebinding
  
-- and one more from large repo
  $ cd "$TESTTMP/large-hg-client"
  $ hg up master_bookmark -q
  $ echo 'largenewcontent' > smallrepofolder/2
  $ hg ci -qm 'after rebinding from large'
  $ hg push -r . --to master_bookmark -q

-- we do not have backsyncer running, so in order to see the change
-- from small repo we need to do a push
  $ cd "$TESTTMP/small-hg-client"
  $ hg up -q master_bookmark
  $ echo 'newcontent' > 3
  $ hg ci -qm 'one more after rebinding'
  $ hg push -r . --to master_bookmark
  pushing rev 9cb648e934be to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (ad40a9a26fbd, 9cb648e934be] (1 commit) to remote bookmark master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 9f6b8b8acc0b
  $ hg log -r master_bookmark
  commit:      9f6b8b8acc0b
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     one more after rebinding
  
  $ hg log -r master_bookmark^
  commit:      d5d1d6d6b445
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     after rebinding from large
  
