# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

Setup config repo:
  $ setup_configerator_configs
  $ INFINITEPUSH_ALLOW_WRITES=true \
  >   INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' \
  >   create_large_small_repo
  Adding synced mapping entry
  $ cd "$TESTTMP/mononoke-config"
  $ enable_pushredirect 1

  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

  $ hg log -R $TESTTMP/small-hg-client -G -T '{node} {desc|firstline}\n'
  @  11f848659bfcf77abd04f947883badd8efa88d26 first post-move commit
  │
  o  fc7ae591de0e714dc3abfb7d4d8aa5f9e400dd77 pre-move commit
  

  $ hg log -R $TESTTMP/large-hg-client -G -T '{node} {desc|firstline}\n'
  @  bfcfb674663c5438027bcde4a7ae5024c838f76a first post-move commit
  │
  o  5a0ba980eee8c305018276735879efba05b3e988 move commit
  │
  o  fc7ae591de0e714dc3abfb7d4d8aa5f9e400dd77 pre-move commit
  

  $ cd "$TESTTMP/small-hg-client"
  $ export REPONAME=small-mon
  $ hg debugapi -e committranslateids -i "[{'Bonsai': '$SMALL_MASTER_BONSAI'}]" -i "'Hg'"
  [{"commit": {"Bonsai": bin("1ba347e63a4bf200944c22ade8dbea038dd271ef97af346ba4ccfaaefb10dd4d")},
    "translated": {"Hg": bin("11f848659bfcf77abd04f947883badd8efa88d26")}}]

  $ hg debugapi -e committranslateids -i "[{'Hg': '11f848659bfcf77abd04f947883badd8efa88d26'}]" -i "'Hg'" -i None -i "'large-mon'"
  [{"commit": {"Hg": bin("11f848659bfcf77abd04f947883badd8efa88d26")},
    "translated": {"Hg": bin("bfcfb674663c5438027bcde4a7ae5024c838f76a")}}]

  $ hg debugapi -e committranslateids -i "[{'Hg': 'bfcfb674663c5438027bcde4a7ae5024c838f76a'}]" -i "'Hg'" -i "'large-mon'"
  [{"commit": {"Hg": bin("bfcfb674663c5438027bcde4a7ae5024c838f76a")},
    "translated": {"Hg": bin("11f848659bfcf77abd04f947883badd8efa88d26")}}]

  $ hg log -r bfcfb67466 -T '{node}\n' --config 'megarepo.transparent-lookup=small-mon large-mon' --config extensions.megarepo=
  pulling 'bfcfb67466' from 'mono:small-mon'
  pull failed: bfcfb67466 not found
  translated bfcfb674663c5438027bcde4a7ae5024c838f76a@large-mon to 11f848659bfcf77abd04f947883badd8efa88d26
  11f848659bfcf77abd04f947883badd8efa88d26

  $ hg log -r large-mon/master_bookmark -T '{node}\n' --config 'megarepo.transparent-lookup=large-mon' --config extensions.megarepo=
  translated bfcfb674663c5438027bcde4a7ae5024c838f76a@large-mon to 11f848659bfcf77abd04f947883badd8efa88d26
  11f848659bfcf77abd04f947883badd8efa88d26

	
# Push a commit to small-repo
  $ cd "$TESTTMP/small-hg-client"
  $ hg up -q master_bookmark
  $ mkdir -p non_path_shifting
  $ echo a > foo
  $ echo b > non_path_shifting/bar
  $ hg ci -Aqm "new small-repo commit"
  $ hg push -r . --to master_bookmark -q
  $ log
  @  new small-repo commit [public;rev=2;a61c0a2e580a] remote/master_bookmark
  │
  o  first post-move commit [public;rev=1;11f848659bfc]
  │
  o  pre-move commit [public;rev=0;fc7ae591de0e]
  $
	
# check the same commit in the large repo
  $ cd "$TESTTMP/large-hg-client"
  $ hg pull -q
  $ hg up -q master_bookmark
  $ log -r master_bookmark
  @  new small-repo commit [public;rev=3;a38079aa2786] remote/master_bookmark
  │
  ~
  $ hg whereami
  a38079aa278633d9e69eb2d90d393b7fec83b09a
  $ LARGE_REPO_MAPPED_COMMIT=$(hg whereami)
	
# Make a large-repo-only commit
  $ echo "large-repo only change" > fbcodefile2
  $ hg add fbcodefile2
  $ hg commit -m "large-repo only commit"
  $ hg push -r . --to master_bookmark -q
  $ hg whereami
  e6c6c1f94d10495f8c94d81ae5f125bd89f82b8a
  $ LARGE_REPO_ONLY_COMMIT=$(hg log -r . -T '{node}')

# Returns an empty array [] for exact lookup behavior, otherwise returns the small repository commit hash.
  $ hg debugapi -e committranslateids -i "[{'Hg': '$LARGE_REPO_MAPPED_COMMIT'}]" -i "'Hg'" -i None -i "'small-mon'"
  [{"commit": {"Hg": bin("a38079aa278633d9e69eb2d90d393b7fec83b09a")},
    "translated": {"Hg": bin("a61c0a2e580a4d7c742858d8ebf1a469a7de0839")}}]
  $ hg debugapi -e committranslateids -i "[{'Hg': '$LARGE_REPO_ONLY_COMMIT'}]" -i "'Hg'" -i None -i "'small-mon'" -i "'exact'"
  []

# Translate both the LARGE_REPO_-MAPPED-COMMIT and LARGE-REPO-ONLY-COMMIT from the small repository
# FIXME: Both LARGE_REPO_ONLY_COMMIT and LARGE_REPO-MAPPED-COMMIT incorrectly mapped the same small commit 
  $ cd "$TESTTMP/small-hg-client"
  $ hg up -q master_bookmark
  $ hg whereami
  a61c0a2e580a4d7c742858d8ebf1a469a7de0839
  $ hg log -r $LARGE_REPO_MAPPED_COMMIT -T '{node}\n' --config 'megarepo.transparent-lookup=small-mon large-mon' --config extensions.megarepo=
  pulling 'a38079aa278633d9e69eb2d90d393b7fec83b09a' from 'mono:small-mon'
  pull failed: a38079aa278633d9e69eb2d90d393b7fec83b09a not found
  translated a38079aa278633d9e69eb2d90d393b7fec83b09a@large-mon to a61c0a2e580a4d7c742858d8ebf1a469a7de0839
  a61c0a2e580a4d7c742858d8ebf1a469a7de0839
  $ hg log -r $LARGE_REPO_ONLY_COMMIT -T '{node}\n' --config 'megarepo.transparent-lookup=small-mon large-mon' --config extensions.megarepo=
  pulling 'e6c6c1f94d10495f8c94d81ae5f125bd89f82b8a' from 'mono:small-mon'
  pull failed: e6c6c1f94d10495f8c94d81ae5f125bd89f82b8a not found
  translated e6c6c1f94d10495f8c94d81ae5f125bd89f82b8a@large-mon to a61c0a2e580a4d7c742858d8ebf1a469a7de0839
  a61c0a2e580a4d7c742858d8ebf1a469a7de0839

