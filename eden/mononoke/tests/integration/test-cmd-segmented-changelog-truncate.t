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
  $ grep segmented_changelog "$TESTTMP/quiet.last.log"
  * SegmentedChangelogSeeder initialized for repository 'repo' (glob)
  * using '*' for head (glob)
  * seeding segmented changelog using idmap version: 1 (glob)
  * idmap version bumped (glob)
  * repo 0: segmented changelog version saved, idmap_version: 1, iddag_version: c2e0b7de* (glob)
  * successfully finished seeding segmented changelog (glob)
  * successfully finished seeding SegmentedChangelog for repository 'repo' (glob)

Run Segmented Changelog Tailer. Nothing to do.

  $ quiet segmented_changelog_tailer --repo repo
  $ grep segmented_changelog "$TESTTMP/quiet.last.log"
  * repo name 'repo' translates to id 0 (glob)
  * repo 0: SegmentedChangelogTailer initialized (glob)
  * repo 0: starting incremental update to segmented changelog (glob)
  * repo 0: bookmark master_bookmark resolved to * (glob)
  * repo 0: segmented changelog already up to date, skipping update to iddag (glob)
  * repo 0: SegmentedChangelogTailer is done (glob)

Truncate down to 1 changeset and then tail in the missing two
  $ quiet mononoke_admin truncate-segmented-changelog $(hg log -T'{node}' -r 'limit(::master_bookmark, 1)')
  $ grep segmented_changelog "$TESTTMP/quiet.last.log"
  * repo 0: segmented changelog version saved, idmap_version: 2, iddag_version: 5fd1e81c* (glob)

Run the tailer again, and see it pull in the commits we removed
  $ quiet segmented_changelog_tailer --repo repo
  $ grep segmented_changelog "$TESTTMP/quiet.last.log"
  * repo name 'repo' translates to id 0 (glob)
  * repo 0: SegmentedChangelogTailer initialized (glob)
  * repo 0: starting incremental update to segmented changelog (glob)
  * repo 0: bookmark master_bookmark resolved to * (glob)
  * repo 0: IdMap updated, IdDag updated (glob)
  * repo 0: segmented changelog version saved, idmap_version: 2, iddag_version: c2e0b7de* (glob)
  * repo 0: successful incremental update to segmented changelog (glob)
  * repo 0: SegmentedChangelogTailer is done (glob)
