# Copyright (c) Meta Platforms, Inc. and affiliates.
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

Run Segmented Changelog Tailer. This seeds the segmented changelog.
  $ segmented_changelog_tailer_once --head master_bookmark --repo repo &>"$TESTTMP/error.log"
  $ grep -e successfully -e segmented_changelog_tailer -e idmap_version "$TESTTMP/error.log"
  * repo name 'repo' translates to id 0 (glob)
  * repo 0: using * for head (glob)
  * repo 0: SegmentedChangelogTailer initialized (glob)
  * Adding hints for repo 0 idmap_version 1 (glob)
  * repo 0 idmap_version 1 has a full set of hints * (glob)
  * repo 0: segmented changelog version saved, idmap_version: 1, iddag_version: * (glob)
  * repo 0: successfully seeded segmented changelog (glob)
  * repo 0: SegmentedChangelogTailer is done (glob)


Now test without head option (tailer will fetch it from config) and with prefetched commits
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > master_bookmark="master_bookmark"
  > CONFIG
  $ dump_public_changeset_entries --out-filename "$TESTTMP/prefetched_commits" &> /dev/null
  $ segmented_changelog_tailer_reseed --repo repo --prefetched-commits-path "$TESTTMP/prefetched_commits" &>"$TESTTMP/error.log"
  $ grep -e successfully -e segmented_changelog_tailer -e idmap_version "$TESTTMP/error.log"
  * reading prefetched commits from $TESTTMP/prefetched_commits (glob)
  * repo name 'repo' translates to id 0 (glob)
  * repo 0: SegmentedChangelogTailer initialized (glob)
  * Adding hints for repo 0 idmap_version 2 (glob)
  * repo 0 idmap_version 2 has a full set of hints * (glob)
  * repo 0: segmented changelog version saved, idmap_version: 2, iddag_version: * (glob)
  * repo 0: successfully seeded segmented changelog (glob)
  * repo 0: SegmentedChangelogTailer is done (glob)

Add a new commit, and see the tailer tail it in properly
  $ cd repo-hg || exit 1
  $ echo "segments" > changelog
  $ hgmn commit -qAm "new"
  $ hg bookmark -f master_bookmark -r tip
  $ cd ..
  $ blobimport repo-hg/.hg repo --derived-data-type fsnodes
  $ quiet segmented_changelog_tailer_once --repo repo
  $ grep 'successful incremental update' "$TESTTMP/quiet.last.log"
  * repo 0: successful incremental update to segmented changelog (glob)

Run Segmented Changelog Tailer. Nothing to do.

  $ quiet segmented_changelog_tailer_once --repo repo
  $ grep 'already up to date' "$TESTTMP/quiet.last.log"
  * repo 0: segmented changelog already up to date, skipping update to iddag (glob)
