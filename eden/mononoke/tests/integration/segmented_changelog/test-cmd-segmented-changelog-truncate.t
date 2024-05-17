# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repository

  $ BLOB_TYPE="blob_files" quiet default_setup

Run Segmented Changelog Tailer to seed the repo.
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > heads_to_include = [
  >    { bookmark = "master_bookmark" },
  > ]
  > CONFIG
  $ quiet segmented_changelog_tailer_reseed --repo repo
  $ grep 'successfully' "$TESTTMP/quiet.last.log"
  * successfully seeded segmented changelog, repo_id: 0 (glob)

Run Segmented Changelog Tailer. Nothing to do.

  $ quiet segmented_changelog_tailer_once --repo repo
  $ grep 'already up to date'  "$TESTTMP/quiet.last.log"
  * segmented changelog already up to date, skipping update to iddag, repo_id: 0 (glob)

Truncate down to 1 changeset and then tail in the missing two
  $ quiet mononoke_admin truncate-segmented-changelog $(hg log -T'{node}' -r 'limit(::master_bookmark, 1)')
  $ grep 'segmented changelog version saved' "$TESTTMP/quiet.last.log"
  * segmented changelog version saved, idmap_version: 2, iddag_version: 5fd1e81c* (glob)

Run the tailer again, and see it pull in the commits we removed
  $ quiet segmented_changelog_tailer_once --repo repo
  $ grep 'successful incremental update' "$TESTTMP/quiet.last.log"
  * successful incremental update to segmented changelog, repo_id: 0 (glob)
