# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ configure modern
  $ enable amend infinitepush infinitepushbackup remotenames
  $ function hgsmall {
  >   REPONAME=small-mon hgedenapi $@
  > }
  $ function hglarge {
  >   REPONAME=large-mon hgedenapi $@
  > }
  $ function globalrev {
  >   (hg log -r . -T '{extras % "{extra}\n"}' | grep global_rev) || echo "no globalrev"
  > }

Let's set config manually, so we can LATER start the sync
  $ GLOBALREVS_PUBLISHING_BOOKMARK=master_bookmark GLOBALREVS_SMALL_REPO_ID=1 REPOID=0 REPONAME=large-mon setup_common_config blob_files
  $ DISALLOW_NON_PUSHREBASE=1 GLOBALREVS_PUBLISHING_BOOKMARK=master_bookmark REPOID=1 REPONAME=small-mon setup_common_config blob_files
  $ large_small_megarepo_config
Start repos, mononoke, clones
  $ large_small_setup
  Adding synced mapping entry
  $ XREPOSYNC=1 start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones
Check small repo
  $ cd "$TESTTMP/small-hg-client"
  $ quiet hgsmall pull -B master_bookmark
Pushrebase a commit, check globalrev
  $ quiet hgsmall prev
  $ mkcommit S_C && mkcommit S_D
  $ hgsmall push --to master_bookmark -q
  $ hgsmall up master_bookmark -q
  $ globalrev
  global_rev=1000147971
  $ mononoke_newadmin convert --from globalrev --to hg -R small-mon 1000147971
  15a08a4c4f68fabe70baea3a18693ec7bca1f260
  $ mononoke_newadmin convert --from hg --to globalrev -R small-mon $(hg log -T{node} -r .)
  1000147971
  $ tglogpnr -r 'public()'
  @  15a08a4c4f68 public 'S_D'  remote/master_bookmark
  │
  o  b5f06114b6cd public 'S_C'
  │
  o  11f848659bfc public 'first post-move commit'
  │
  o  fc7ae591de0e public 'pre-move commit'
  
Look at large repo
  $ cd "$TESTTMP/large-hg-client"
  $ quiet hglarge pull -B master_bookmark
  $ mkcommit L1
  $ hglarge push --to master_bookmark -q
  $ hglarge up master_bookmark -q
  $ quiet mononoke_x_repo_sync 1 0 tail --bookmark-regex "master_bookmark" --catch-up-once
  $ hg pull -B master_bookmark -q && tglogpnr -r 'public()'
  o  1ecea6f1ca78 public 'S_D'  remote/master_bookmark
  │
  o  ccfba9c1c7e0 public 'S_C'
  │
  @  04dcad07512b public 'L1'
  │
  o  bfcfb674663c public 'first post-move commit'
  │
  o  5a0ba980eee8 public 'move commit'
  │
  o  fc7ae591de0e public 'pre-move commit'
  
Large repo commit has no globalrev
  $ globalrev
  no globalrev
Small repo forward synced commit does (not on DB)
This proves globalrevs continue working with forward sync
  $ hglarge up master_bookmark -q
  $ globalrev
  global_rev=1000147971
  $ mononoke_newadmin convert --from globalrev --to hg -R large-mon 1000147971
  Error: globalrev-bonsai mapping not found for 1000147971
  [1]
  $ mononoke_newadmin convert --from hg --to globalrev -R large-mon $(hg log -T{node} -r .)
  Error: bonsai-globalrev mapping not found for 5501b03f364bab00acc28a612577692c4e1a5303efe812826052926777a462d5
  [1]

Lets change the sync direction
  $ LAST_UPDATE=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select max(id) from bookmarks_update_log where repo_id=$REPOIDLARGE")
  $ quiet mononoke_newadmin mutable-counters --repo-id $REPOIDSMALL set backsync_from_$REPOIDLARGE $LAST_UPDATE
  $ enable_pushredirect $REPOIDSMALL

Do a few commits on large repo. They have globalrevs.
  $ mkcommit large_only
  $ echo hello > smallrepofolder/new_file_from_large.txt
  $ hglarge addremove -q && hglarge commit -m "commit_from_large_to_small"
  $ hglarge push --to master_bookmark -q && hglarge up master_bookmark -q
  $ globalrev
  global_rev=1000147972

See commit on small repo. It was imported with globalrev and we can query globalrevs on small repo.
  $ cd "$TESTTMP/small-hg-client"
  $ quiet backsync_large_to_small -q
  $ hgsmall pull -B master_bookmark -q && hgsmall up master_bookmark -q && tglogpnr -r 'public()'
  @  b92328759f69 public 'commit_from_large_to_small'  remote/master_bookmark
  │
  o  15a08a4c4f68 public 'S_D'
  │
  o  b5f06114b6cd public 'S_C'
  │
  o  11f848659bfc public 'first post-move commit'
  │
  o  fc7ae591de0e public 'pre-move commit'
  
  $ globalrev
  global_rev=1000147972
  $ mononoke_newadmin convert --from globalrev --to hg -R small-mon 1000147972
  b92328759f69a2627d676c1074e25561325890bc
  $ mononoke_newadmin convert --from hg --to globalrev -R small-mon $(hg log -T{node} -r .)
  1000147972
Push through small repo.
  $ mkcommit commit_from_small
  $ hgsmall push --to master_bookmark -q && hgsmall up master_bookmark -q
  $ globalrev
  global_rev=1000147973
  $ mononoke_newadmin convert --from globalrev --to hg -R small-mon 1000147973
  f378fa07a8e3f0e0a755df2112896914325abbe5
  $ mononoke_newadmin convert --from hg --to globalrev -R small-mon $(hg log -T{node} -r .)
  1000147973

