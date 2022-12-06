# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repository

  $ setup_common_config "$@"
  $ LOG_FILE="$TESTTMP/log_file"

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
  $ quiet segmented_changelog_tailer_once --log-level TRACE --head master_bookmark --repo repo
  $ grep -e "repo_id: 0" -e "segmented_changelog_tailer" "$TESTTMP/quiet.last.log"
  * repo name 'repo' translates to id 0 (glob)
  * changeset resolved as: *, repo_id: 0 (glob)
  * using * for head, repo_id: 0 (glob)
  * SegmentedChangelogTailer initialized, repo_id: 0 (glob)
  * woke up to update, repo_id: 0 (glob)
  * starting incremental update to segmented changelog, repo_id: 0 (glob)
  * iddag initialized, it covers 0 ids, repo_id: 0 (glob)
  * segmented changelog idmap instantiated - version: 1, repo_id: 0 (glob)
  * starting the actual update, repo_id: 0 (glob)
  * Adding hints for idmap_version 1, repo_id: 0 (glob)
  * idmap_version 1 has a full set of hints *, repo_id: 0 (glob)
  * flushing 3 in-memory IdMap entries to SQL, repo_id: 0 (glob)
  * IdMap updated, IdDag updated, repo_id: 0 (glob)
  * segmented changelog version saved, idmap_version: 1, iddag_version: *, repo_id: 0 (glob)
  * successfully seeded segmented changelog, repo_id: 0 (glob)
  * SegmentedChangelogTailer is done, repo_id: 0 (glob)


Now test without head option (tailer will fetch it from config) and with prefetched commits
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > heads_to_include = [
  >    { all_public_bookmarks_except = ["master_bookmark"] },
  > ]
  > extra_heads_to_include_in_background_jobs = [
  >    {  bookmark = "master_bookmark" },
  > ]
  > CONFIG
  $ quiet mononoke_newadmin dump-changesets -R repo --out-filename "$TESTTMP/prefetched_commits" fetch-public
  $ quiet segmented_changelog_tailer_reseed --log-level TRACE --scuba-dataset "file://$LOG_FILE" --repo repo --prefetched-commits-path "$TESTTMP/prefetched_commits"
  $ grep -e "repo_id: 0" -e "segmented_changelog_tailer" "$TESTTMP/quiet.last.log"
  * reading prefetched commits from $TESTTMP/prefetched_commits (glob)
  * repo name 'repo' translates to id 0 (glob)
  * Using the following segmented changelog heads: [AllPublicBookmarksExcept([BookmarkName { bookmark: "master_bookmark" }]), Bookmark(BookmarkName { bookmark: "master_bookmark" })], repo_id: 0 (glob)
  * SegmentedChangelogTailer initialized, repo_id: 0 (glob)
  * woke up to update, repo_id: 0 (glob)
  * starting incremental update to segmented changelog, repo_id: 0 (glob)
  * iddag initialized, it covers 0 ids, repo_id: 0 (glob)
  * segmented changelog idmap instantiated - version: 2, repo_id: 0 (glob)
  * starting the actual update, repo_id: 0 (glob)
  * Adding hints for idmap_version 2, repo_id: 0 (glob)
  * idmap_version 2 has a full set of hints *, repo_id: 0 (glob)
  * flushing 3 in-memory IdMap entries to SQL, repo_id: 0 (glob)
  * IdMap updated, IdDag updated, repo_id: 0 (glob)
  * segmented changelog version saved, idmap_version: 2, iddag_version: *, repo_id: 0 (glob)
  * successfully seeded segmented changelog, repo_id: 0 (glob)
  * SegmentedChangelogTailer is done, repo_id: 0 (glob)

Add a new commit, and see the tailer tail it in properly
  $ cd repo-hg || exit 1
  $ echo "segments" > changelog
  $ hgmn commit -qAm "new"
  $ hg bookmark -f master_bookmark -r tip
  $ cd ..
  $ blobimport repo-hg/.hg repo --derived-data-type fsnodes
  $ quiet segmented_changelog_tailer_once --scuba-dataset "file://$LOG_FILE"  --repo repo
  $ grep "repo_id: 0" "$TESTTMP/quiet.last.log"
  * Using the following segmented changelog heads: [AllPublicBookmarksExcept([BookmarkName { bookmark: "master_bookmark" }]), Bookmark(BookmarkName { bookmark: "master_bookmark" })], repo_id: 0 (glob)
  * SegmentedChangelogTailer initialized, repo_id: 0 (glob)
  * starting incremental update to segmented changelog, repo_id: 0 (glob)
  * iddag initialized, it covers 3 ids, repo_id: 0 (glob)
  * starting the actual update, repo_id: 0 (glob)
  * Adding hints for idmap_version 2, repo_id: 0 (glob)
  * idmap_version 2 has a full set of hints *, repo_id: 0 (glob)
  * IdMap updated, IdDag updated, repo_id: 0 (glob)
  * segmented changelog version saved, idmap_version: 2, iddag_version: *, repo_id: 0 (glob)
  * successful incremental update to segmented changelog, repo_id: 0 (glob)
  * SegmentedChangelogTailer is done, repo_id: 0 (glob)

Check scuba logging
  $ cat "$LOG_FILE" | summarize_scuba_json "segmented_changelog_tailer_update" \
  >     .normal.log_tag | jq -s  length
  2

Run Segmented Changelog Tailer. Nothing to do.

  $ quiet segmented_changelog_tailer_once --repo repo
  $ grep 'already up to date' "$TESTTMP/quiet.last.log"
  * segmented changelog already up to date, skipping update to iddag, repo_id: 0 (glob)
