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
  $ grep 'successfully finished' "$TESTTMP/quiet.last.log"
  * successfully finished seeding segmented changelog (glob)
  * successfully finished seeding SegmentedChangelog for repository 'repo' (glob)

Now run with prefetched changesets
  $ dump_public_changeset_entries --out-filename "$TESTTMP/prefetched_commits" &> /dev/null
  $ quiet segmented_changelog_seeder --head=master_bookmark --prefetched-commits-path="$TESTTMP/prefetched_commits"
  $ grep 'successfully finished' "$TESTTMP/quiet.last.log"
  * successfully finished seeding segmented changelog (glob)
  * successfully finished seeding SegmentedChangelog for repository 'repo' (glob)
