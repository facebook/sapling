# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repository

  $ BLOB_TYPE="blob_files" quiet default_setup

Run Segmented Changelog Seeder.

  $ quiet segmented_changelog_seeder --head=master_bookmark
  $ grep segmented_changelog "$TESTTMP/quiet.last.log"
  * SegmentedChangelogSeeder initialized for repository 'repo' (glob)
  * using '*' for head (glob)
  * seeding segmented changelog using idmap version: 1 (glob)
  * idmap version bumped (glob)
  * repo 0: segmented changelog version saved, idmap_version: 1, iddag_version: * (glob)
  * successfully finished seeding segmented changelog (glob)
  * successfully finished seeding SegmentedChangelog for repository 'repo' (glob)

Now run with prefetched changesets
  $ dump_public_changeset_entries --out-filename "$TESTTMP/prefetched_commits" &> /dev/null
  $ quiet segmented_changelog_seeder --head=master_bookmark --prefetched-commits-path="$TESTTMP/prefetched_commits"
  $ grep segmented_changelog "$TESTTMP/quiet.last.log"
  * reading prefetched commits from $TESTTMP/prefetched_commits (glob)
  * SegmentedChangelogSeeder initialized for repository 'repo' (glob)
  * using '*' for head (glob)
  * seeding segmented changelog using idmap version: 2 (glob)
  * idmap version bumped (glob)
  * repo 0: segmented changelog version saved, idmap_version: 2, iddag_version: * (glob)
  * successfully finished seeding segmented changelog (glob)
  * successfully finished seeding SegmentedChangelog for repository 'repo' (glob)
