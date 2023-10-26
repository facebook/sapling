# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

In this test we ensure that a merge into the large repo that doesn't involve
**any** small repo paths doesn't result in merge commit when backsynced to small repo.

  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ export COMMIT_SCRIBE_CATEGORY=mononoke_commits
  $ export BOOKMARK_SCRIBE_CATEGORY=mononoke_bookmark

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

  $ testtool_drawdag --print-hg-hashes -R large-mon --no-default-files <<'EOF'
  >        M1
  >       /  \
  >     A1    |  # modify: A1 unrelatedfolder/newrepo "content"
  >           E1 
  > EOF
  A1=dc5b1bb7a82bef93a47a92b4f9ac5fb54597cd78
  E1=7614fd547c87f4952b0196834ce4dbee6eaf4eed
  M1=16e478e066f462f5f764bd35e49f9847106d0002

Push a M1 to a large repo
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -qr $M1
  $ REPONAME=large-mon hgmn up -q master_bookmark^
  $ hg merge -r "$M1" -q
  $ hg ci -m 'merge commit in large repo #1'
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q

Backsync to a small repo
  $ quiet_grep "syncing bookmark" -- backsync_large_to_small
  * syncing bookmark master_bookmark to * (glob)

Skip empty commits option
  $ merge_tunables <<EOF
  > {
  > "killswitches_by_repo": {
  >   "large-mon": {
  >      "cross_repo_skip_backsyncing_ordinary_empty_commits": true
  >    }
  >   }
  > }
  > EOF

  $ testtool_drawdag --print-hg-hashes -R large-mon --no-default-files <<'EOF'
  >        M2
  >       /  \
  >     A2    |  # modify: A2 unrelatedfolder2/newrepo2 "content"
  >           E2
  > EOF
  A2=04112d407dfa381d9f6d0ebbab5f14a5be25bd07
  E2=012af04588fd9e7970b2a00d32bbdd6c62452482
  M2=c24e2384288c1aa49554338aa4f48f579655bb50

Push a M2 to a large repo
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -qr $M2
  $ REPONAME=large-mon hgmn up -q master_bookmark^
  $ hg merge -r "$M2" -q
  $ hg ci -m 'merge commit in large repo #2 - should be non-merge in small repo'
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q

Backsync to a small repo
  $ quiet_grep "syncing bookmark" -- backsync_large_to_small
  * syncing bookmark master_bookmark to * (glob)
  $ flush_mononoke_bookmarks

Pull from a small repo. Check that both merges are synced
although the second one became non-merge commit
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ log -r :
  o  merge commit in large repo #2 - should be non-merge in small repo [public;rev=5;5be69a1c5de7] default/master_bookmark
  │
  o    merge commit in large repo #1 [public;rev=4;8e94c6a96669]
  ├─╮
  │ o  M1 [public;rev=3;1b442d07b913]
  │ │
  │ o  E1 [public;rev=2;7614fd547c87]
  │
  @  first post-move commit [public;rev=1;11f848659bfc]
  │
  o  pre-move commit [public;rev=0;fc7ae591de0e]
  $
