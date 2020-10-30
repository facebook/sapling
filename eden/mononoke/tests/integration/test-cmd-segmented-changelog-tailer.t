# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repository

  $ setup_common_config "$@"

  $ cat >> "$HGRCPATH" <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > EOF

  $ hg init repo-hg
  $ cd repo-hg || exit 1
  $ setup_hg_server
  $ drawdag <<EOF
  > F
  > |
  > E
  > |\
  > C D
  > |/
  > B
  > |
  > A
  > EOF

  $ hg bookmark master_bookmark -r tip
  $ cd ..
  $ blobimport repo-hg/.hg repo --derived-data-type fsnodes

  $ quiet default_setup_blobimport "blob_files"

Run Segmented Changelog Tailer. Error because there was no seeding.

  $ segmented_changelog_tailer --track-bookmark=master_bookmark &>"$TESTTMP/error.log"
  [1]
  $ grep seeding "$TESTTMP/error.log"
  * maybe it needs seeding (glob)

Seed repository.
  $ quiet segmented_changelog_seeder --head=$A

Actually run Segmented Changelog Tailer.

  $ quiet segmented_changelog_tailer --track-bookmark=master_bookmark --once
  $ grep segmented_changelog "$TESTTMP/quiet.last.log"
  * SegmentedChangelogTailer initialized for repository 'repo' (glob)
  * starting incremental update to segmented changelog (glob)
  * base idmap version: 1; base iddag version: 3ecf193f* (glob)
  * base dag loaded successfully (glob)
  * bookmark master_bookmark resolved to * (glob)
  * IdMap updated, IdDag updated (glob)
  * IdDag rebuilt (glob)
  * success - new iddag saved, idmap_version: 1, iddag_version: e159f327* (glob)
  * SegmentedChangelogTailer is done for repo repo (glob)

Run Segmented Changelog Tailer. Nothing to do.

  $ quiet segmented_changelog_tailer --track-bookmark=master_bookmark --once
  $ grep segmented_changelog "$TESTTMP/quiet.last.log"
  * SegmentedChangelogTailer initialized for repository 'repo' (glob)
  * starting incremental update to segmented changelog (glob)
  * base idmap version: 1; base iddag version: e159f327* (glob)
  * base dag loaded successfully (glob)
  * bookmark master_bookmark resolved to * (glob)
  * dag already up to date, skipping update to iddag (glob)
  * SegmentedChangelogTailer is done for repo repo (glob)
