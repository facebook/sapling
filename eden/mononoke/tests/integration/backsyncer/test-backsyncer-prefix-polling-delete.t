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

Start the backsyncer with bookmark-prefix polling enabled.

  $ backsync_large_to_small_forever --just-knob scm/mononoke:backsyncer_prefix_polling=true

Create a large-repo bookmark under the configured prefix and wait for it to be
projected into the small repo with the prefix removed.

  $ cd "$TESTTMP/large-hg-client"
  $ hg up -q master_bookmark
  $ mkdir -p smallrepofolder
  $ echo prefixed > smallrepofolder/prefixed
  $ hg ci -Aqm "prefixed bookmark change"
  $ hg push -r . --to bookprefix/feature --create -q
  $ quiet wait_for_bookmark_move_to_commit "prefixed bookmark change" small-mon feature

Delete the source bookmark directly in the large repo. It is now absent from
the authoritative bookmarks table, so the source/target reconciliation loop
must still remove the projected small-repo bookmark.

  $ PREV_FEATURE_VALUE=$(get_bookmark_value_bonsai small-mon feature)
  $ quiet mononoke_admin bookmarks -R large-mon delete bookprefix/feature --force-megarepo
  $ quiet wait_for_bookmark_move_away_bonsai small-mon feature "$PREV_FEATURE_VALUE"

Only the configured common bookmark remains in the small repo.

  $ mononoke_admin bookmarks -R small-mon list | cut -d " " -f2 | sort
  master_bookmark
