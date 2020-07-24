# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'

backfill derived data
  $ DERIVED_DATA_TYPE="fsnodes"
  $ backfill_derived_data prefetch-commits --out-filename "$TESTTMP/prefetched_commits"
  * using repo "repo" repoid RepositoryId(0) (glob)

  $ backfill_derived_data backfill --prefetched-commits-path "$TESTTMP/prefetched_commits" "$DERIVED_DATA_TYPE" --limit 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * reading all changesets for: RepositoryId(0) (glob)
  * starting deriving data for 1 changesets (glob)
  * 1/1 estimate:* speed:* mean_speed:* (glob)
  $ hg log -r 0 -T '{node}'
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 (no-eol)
  $ mononoke_admin --log-level ERROR derived-data exists "$DERIVED_DATA_TYPE" 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  Derived: 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec
  $ backfill_derived_data backfill --prefetched-commits-path "$TESTTMP/prefetched_commits" "$DERIVED_DATA_TYPE" --skip-changesets 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * reading all changesets for: RepositoryId(0) (glob)
  * starting deriving data for 2 changesets (glob)
  * 2/2 estimate:0.00ns speed:* mean_speed:* (glob)

  $ mononoke_admin --log-level ERROR derived-data exists "$DERIVED_DATA_TYPE" master_bookmark
  Derived: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd

  $ backfill_derived_data single c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd "$DERIVED_DATA_TYPE"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: * (glob)
  * derived fsnodes in * (glob)
  $ backfill_derived_data single c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd --all-types 2>&1 | grep derived | count_stdin_lines
  8
