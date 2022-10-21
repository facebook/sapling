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

  $ init_large_small_repo
  Adding synced mapping entry
  Starting Mononoke server

-- normal pushrebase with one commit
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


-- unbind repositories and wait until it propagates
  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "1": {
  >      "draft_push": false,
  >      "public_push": false
  >    }
  >   }
  > }
  > EOF
  $ force_update_configerator

-- do a push from small repo, make sure it is not pushredirected to large repo
  $ cd "$TESTTMP/small-hg-client"
  $ echo 2_unbound > 2 && hg ci -q -m unbound_commit
  $ echo 3 > 3 && hg addremove 3 && hg ci -m 'first unbound commit'
  $ echo 4 > 4 && hg addremove 4 && hg ci -m 'second unbound commit'
  $ SMALL_NODE="$(hg log -r tip -T '{node}')"
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark
  $ cd "$TESTTMP"/large-hg-client
  $ REPONAME=large-mon hgmn pull -q &> /dev/null
  $ log -r master_bookmark
  o  newcommit [public;rev=3;819e91b238b7] default/master_bookmark
  │
  ~
  $ REPONAME=large-mon hgmn st --change master_bookmark
  A smallrepofolder/2

-- do a push from large repo as well
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn up master_bookmark -q
  $ echo 'largerepocontent' > smallrepofolder/2
  $ hg ci -m 'large repo unbound commit'
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark 2>&1 | grep updating
  updating bookmark master_bookmark
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
  $ HG_CS_ID="$(REPOID=0 mononoke_admin convert --from bonsai --to hg 146b951933c6d1554a377d733af183659f61794da5c6537c5de68e52acd5e949 2> /dev/null)"
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -r "$HG_CS_ID"
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/large-mon
  searching for changes
  adding changesets
  adding manifests
  adding file changes

-- Step 3. now do merge in the large repo that fixed working copy and push it
  $ REPONAME=large-mon hgmn up master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
-- note - --tool ':local' is used only in tests,
-- you need something smarter in prod!
  $ REPONAME=large-mon hgmn merge "$HG_CS_ID" --tool ':local'
  2 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ REPONAME=large-mon hgmn ci -m 'rebinding'
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q
  $ LARGE_REBINDING="$(hg log -r master_bookmark -T '{node}')"

-- Step 4. create a commit that fixes working copy in the small repo and push it
  $ cd "$TESTTMP/small-hg-client"
  $ echo 'largerepocontent' > 2
  $ hg ci -qm 'rebinding'
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark -q
  $ SMALL_REBINDING="$(hg log -r master_bookmark -T '{node}')"

-- Step 5. mark commits that fix working copy as rewritten
  $ megarepo_tool_multirepo --source-repo-id 1 --target-repo-id 0 check-push-redirection-prereqs "$SMALL_REBINDING" "$LARGE_REBINDING" test_version 2>&1 | grep 'all is well!'
  * all is well! (glob)
  $ mononoke_admin_source_target 0 1 crossrepo insert rewritten \
  > --source-hash "$LARGE_REBINDING" --target-hash "$SMALL_REBINDING" --version-name test_version 2>&1 | grep 'successfully inserted'
  * successfully inserted rewritten mapping entry (glob)

-- Step 6. Rebind repositories and wait until it propagates
  $ mononoke_admin_source_target 0 1 crossrepo pushredirection prepare-rollout &> /dev/null
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
  $ force_update_configerator

-- Verify it works fine
-- Do a new push from small repo from one of the
-- unbound commits
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q "$SMALL_NODE"
  $ echo 'newfile' > newfile
  $ hg add newfile
  $ hg ci -qm 'after rebinding'
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark -q
  $ hg log -r master_bookmark
  commit:      ad40a9a26fbd
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     after rebinding
  
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ hg log -r master_bookmark
  commit:      57b52edb15eb
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     after rebinding
  
-- and one more from large repo
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn up master_bookmark -q
  $ echo 'largenewcontent' > smallrepofolder/2
  $ hg ci -qm 'after rebinding from large'
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q

-- we do not have backsyncer running, so in order to see the change
-- from small repo we need to do a push
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ echo 'newcontent' > 3
  $ hg ci -qm 'one more after rebinding'
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark
  pushing rev 9cb648e934be to destination mononoke://$LOCALIP:$LOCAL_PORT/small-mon bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
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
  
