# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repository

  $ BLOB_TYPE="blob_files" quiet default_setup

Run Segmented Changelog Builder.

  $ quiet segmented_changelog_seeder --head-bookmark=master_bookmark
  $ grep segmented_changelog "$TESTTMP/quiet.last.log"
  * SegmentedChangelogBuilder initialized for repository 'repo' (glob)
  * resolved bookmark 'master_bookmark' * (glob)
  * seeding segmented changelog using idmap version: 1 (glob)
  * loaded 3 changesets (glob)
  * finished building dag, head '*' has assigned vertex '2' (glob)
  * finished writing dag bundle and updating metadata* (glob)
  * successfully finished seeding SegmentedChangelog for repository 'repo' (glob)

