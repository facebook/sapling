# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repository

  $ BLOB_TYPE="blob_files" quiet default_setup

Run Segmented Changelog Seeder.
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > master_bookmark="master_bookmark"
  > CONFIG
  $ quiet segmented_changelog_seeder --head=master_bookmark
  $ grep 'successfully finished' "$TESTTMP/quiet.last.log"
  * successfully finished seeding segmented changelog (glob)
  * successfully finished seeding SegmentedChangelog for repository 'repo' (glob)

Run Segmented Changelog Tailer. Nothing to do.

  $ quiet segmented_changelog_tailer --repo repo
  $ grep 'already up to date'  "$TESTTMP/quiet.last.log"
  * repo 0: segmented changelog already up to date, skipping update to iddag (glob)

Truncate down to 1 changeset and then tail in the missing two
  $ quiet mononoke_admin truncate-segmented-changelog $(hg log -T'{node}' -r 'limit(::master_bookmark, 1)')
  $ grep 'segmented changelog version saved' "$TESTTMP/quiet.last.log"
  * repo 0: segmented changelog version saved, idmap_version: 2, iddag_version: 5fd1e81c* (glob)

Run the tailer again, and see it pull in the commits we removed
  $ quiet segmented_changelog_tailer --repo repo
  $ grep 'successful incremental update' "$TESTTMP/quiet.last.log"
  * repo 0: successful incremental update to segmented changelog (glob)
