# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

The test is to  demonstrates the importance of exact mapping in cross-repo commit 
translation . The test demonstrates how two different commits in fbsoure/www and fbsource/fbcode respectively 
can translate to a same www commit without exact mapping enabled

This is a fork of test-cross-repo-commit-sync-live.t that brings the via-extra mode
to be fully able to deal with mapping changes regardless of sync direction. I will
replace that file once fully fixed.
  $ export LARGE_REPO_ID=0
  $ export SMALL_REPO_ID=1
  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:cross_repo_skip_backsyncing_ordinary_empty_commits": true
  >   }
  > }
  > EOF

-- Init Mononoke thingies
  $ create_large_small_repo
  Adding synced mapping entry
  $ setup_configerator_configs
  $ enable_pushredirect 1 false false
  $ XREPOSYNC=1 start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

-- Start up the sync job in the background
  $ mononoke_x_repo_sync_forever $REPOIDSMALL $REPOIDLARGE

Before the change
-- push to a small repo
  $ cd "$TESTTMP/small-hg-client"
  $ hg up -q master_bookmark
  $ mkdir -p non_path_shifting
  $ echo a > foo
  $ echo b > non_path_shifting/bar
  $ hg ci -Aqm "small repo commit"
  $ hg push -r . --to master_bookmark -q
  $ log
  @  small repo commit [public;rev=2;fe2cc102ee42] remote/master_bookmark
  │
  o  first post-move commit [public;rev=1;11f848659bfc]
  │
  o  pre-move commit [public;rev=0;fc7ae591de0e]
  $

-- wait a little to give sync job some time to catch up
  $ wait_for_xrepo_sync 2
  $ flush_mononoke_bookmarks

-- check the same commit in the large repo
  $ cd "$TESTTMP/large-hg-client"
  $ hg pull -q
  $ hg up -q master_bookmark
  $ log -r master_bookmark
  @  small repo commit [public;rev=3;733eb6ff5cba] remote/master_bookmark
  │
  ~
  $ LARGE_REPO_MAPPED_COMMIT=$(hg whereami)


# Make an large-repo-only commit
  $ echo "large-repo only change" > fbcodefile2
  $ hg add fbcodefile2
  $ hg commit -m "large repo only commit"
  $ hg push -r . --to master_bookmark -q
  $ LARGE_REPO_ONLY_COMMIT=$(hg log -r . -T '{node}')

# Show the initial small commit mapped to large repo and the second commit made in large repo 
# point to the same small repo commit
# No lookup behavior specified; defaults to None
  $ hg debugapi -e committranslateids -i "[{'Hg': '$LARGE_REPO_ONLY_COMMIT'}]" -i "'Hg'" -i "'large-mon'" -i "'small-mon'"
  [{"commit": {"Hg": bin("54ab50dcc6a97355f6ccbe8bdcd64c2ebb7d82d0")},
    "translated": {"Hg": bin("fe2cc102ee42147f9f2f2095f649efe7aa559f0d")}}]
# "equivalent" lookup behavior: maps to a commit in the large repository
  $ hg debugapi -e committranslateids -i "[{'Hg': '$LARGE_REPO_ONLY_COMMIT'}]" -i "'Hg'" -i "'large-mon'" -i "'small-mon'" -i "'equivalent'"
  [{"commit": {"Hg": bin("54ab50dcc6a97355f6ccbe8bdcd64c2ebb7d82d0")},
    "translated": {"Hg": bin("fe2cc102ee42147f9f2f2095f649efe7aa559f0d")}}]
# Explicitly passing None for lookup behavior; maps to a commit in the large repository
  $ hg debugapi -e committranslateids -i "[{'Hg': '$LARGE_REPO_ONLY_COMMIT'}]" -i "'Hg'" -i "'large-mon'" -i "'small-mon'" -i None
  [{"commit": {"Hg": bin("54ab50dcc6a97355f6ccbe8bdcd64c2ebb7d82d0")},
    "translated": {"Hg": bin("fe2cc102ee42147f9f2f2095f649efe7aa559f0d")}}]
# "exact" lookup behavior: returns only the exact matching commit, otherwise an empty list
  $ hg debugapi -e committranslateids -i "[{'Hg': '$LARGE_REPO_ONLY_COMMIT'}]" -i "'Hg'" -i "'large-mon'" -i "'small-mon'" -i "'exact'"
  []
# Invalid lookup behavior: 'random' string passed as argument 
  $ hg debugapi -e committranslateids -i "[{'Hg': '$LARGE_REPO_ONLY_COMMIT'}]" -i "'Hg'" -i "'large-mon'" -i "'small-mon'" -i "'random'"
  abort: server responded 400 Bad Request for https://localhost:*/edenapi/large-mon/commit/translate_id: {"message":"invalid request: invalid lookup behavior * (glob)
      "x-request-id": "*", (glob)
      "content-type": "application/json",
      "x-load": "1",
      "server": "edenapi_server",
      "x-mononoke-host": * (glob)
      "date": * (glob)
      "content-length": "*", (glob)
  }
  [255]
# x_repo_lookup
  $ x_repo_lookup large-mon small-mon $LARGE_REPO_ONLY_COMMIT "exact"
  []
  $ x_repo_lookup large-mon small-mon $LARGE_REPO_MAPPED_COMMIT "equivalent"
  fe2cc102ee42147f9f2f2095f649efe7aa559f0d
  $ x_repo_lookup large-mon small-mon $LARGE_REPO_MAPPED_COMMIT None
  fe2cc102ee42147f9f2f2095f649efe7aa559f0d
