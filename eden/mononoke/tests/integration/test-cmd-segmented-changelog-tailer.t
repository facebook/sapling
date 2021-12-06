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

  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > master_bookmark="master_bookmark"
  > CONFIG
  $ segmented_changelog_tailer --repo repo &>"$TESTTMP/error.log"
  [1]
  $ grep seeded "$TESTTMP/error.log"
  * maybe repo is not seeded (glob)

Seed repository.
  $ quiet segmented_changelog_seeder --head=$A

Actually run Segmented Changelog Tailer.

  $ quiet segmented_changelog_tailer --repo repo
  $ grep 'successful incremental update' "$TESTTMP/quiet.last.log"
  * repo 0: successful incremental update to segmented changelog (glob)

Run Segmented Changelog Tailer. Nothing to do.

  $ quiet segmented_changelog_tailer --repo repo
  $ grep 'already up to date' "$TESTTMP/quiet.last.log"
  * repo 0: segmented changelog already up to date, skipping update to iddag (glob)
