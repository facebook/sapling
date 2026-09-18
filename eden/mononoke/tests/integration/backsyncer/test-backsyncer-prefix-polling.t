# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ create_large_small_repo
  Adding synced mapping entry
  $ setup_configerator_configs
  $ enable_pushredirect 1
  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

Start the backsyncer with prefix polling explicitly enabled. The shared test
JustKnobs file keeps it disabled for every existing test.

  $ backsync_large_to_small_forever --just-knob scm/mononoke:backsyncer_prefix_polling=true

An unrelated large-repo bookmark must not be projected into the small repo.

  $ cd "$TESTTMP/large-hg-client"
  $ hg up -q master_bookmark
  $ mkdir -p smallrepofolder
  $ echo unrelated > smallrepofolder/unrelated
  $ hg ci -Aqm "unrelated bookmark change"
  $ hg push -r . --to unrelated --create -q

A bookmark under the configured prefix is discovered and synced with the
prefix removed.

  $ echo prefixed > smallrepofolder/prefixed
  $ hg ci -Aqm "prefixed bookmark change"
  $ hg push -r . --to bookprefix/feature --create -q
  $ quiet wait_for_bookmark_move_to_commit "prefixed bookmark change" small-mon feature

The configured common bookmark is discovered even though it does not have the
small repository's prefix.

  $ echo common > smallrepofolder/common
  $ hg ci -Aqm "common bookmark change"
  $ hg push -r . --to master_bookmark -q
  $ quiet wait_for_bookmark_move_to_commit "common bookmark change" small-mon master_bookmark

Only the configured prefix and common bookmark were projected.

  $ mononoke_admin bookmarks -R small-mon list | cut -d " " -f2 | sort
  feature
  master_bookmark
