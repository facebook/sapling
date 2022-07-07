# Copyright (c) Meta Platforms, Inc. and affiliates.
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
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'

backfill derived data
  $ DERIVED_DATA_TYPE="fsnodes"
  $ quiet mononoke_newadmin dump-changesets -R repo --out-filename "$TESTTMP/prefetched_commits" fetch-public

  $ backfill_derived_data backfill --prefetched-commits-path "$TESTTMP/prefetched_commits" "$DERIVED_DATA_TYPE" --limit 1
  *] enabled stdlog with level: Error (set RUST_LOG to configure) (glob)
  *] Initializing tunables: * (glob)
  *] Setting up derived data command for repo repo (glob)
  *] Completed derived data command setup for repo repo (glob)
  *] Initiating derived data command execution for repo repo* (glob)
  * using repo "repo" repoid RepositoryId(0)* (glob)
  *] Initializing CfgrLiveCommitSyncConfig, repo: repo (glob)
  *] Initialized PushRedirect configerator config, repo: repo (glob)
  *] Initialized all commit sync versions configerator config, repo: repo (glob)
  *] Done initializing CfgrLiveCommitSyncConfig, repo: repo (glob)
  * reading all changesets for: RepositoryId(0)* (glob)
  * starting deriving data for 1 changesets* (glob)
  * starting batch of 1 from 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec* (glob)
  * warmup of 1 changesets complete* (glob)
  *] backfill fsnodes batch from 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec to 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec* (glob)
  * 1/1 * (glob)
  *] Finished derived data command execution for repo repo* (glob)
  $ hg log -r "min(all())" -T '{node}'
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 (no-eol)
  $ mononoke_admin --log-level ERROR derived-data exists "$DERIVED_DATA_TYPE" 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  Derived: 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec
  $ backfill_derived_data backfill --prefetched-commits-path "$TESTTMP/prefetched_commits" "$DERIVED_DATA_TYPE" --skip-changesets 1
  *] enabled stdlog with level: Error (set RUST_LOG to configure) (glob)
  *] Initializing tunables: * (glob)
  *] Setting up derived data command for repo repo (glob)
  *] Completed derived data command setup for repo repo (glob)
  *] Initiating derived data command execution for repo repo* (glob)
  * using repo "repo" repoid RepositoryId(0)* (glob)
  *] Initializing CfgrLiveCommitSyncConfig, repo: repo (glob)
  *] Initialized PushRedirect configerator config, repo: repo (glob)
  *] Initialized all commit sync versions configerator config, repo: repo (glob)
  *] Done initializing CfgrLiveCommitSyncConfig, repo: repo (glob)
  * reading all changesets for: RepositoryId(0)* (glob)
  * starting deriving data for 2 changesets* (glob)
  * starting batch of 2 from 459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66* (glob)
  * warmup of 2 changesets complete* (glob)
  *] backfill fsnodes batch from 459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66 to c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd* (glob)
  * 2/2 * (glob)
  *] Finished derived data command execution for repo repo* (glob)

  $ mononoke_admin --log-level ERROR derived-data exists "$DERIVED_DATA_TYPE" master_bookmark
  Derived: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd

  $ backfill_derived_data single c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd "$DERIVED_DATA_TYPE"
  *] enabled stdlog with level: Error (set RUST_LOG to configure) (glob)
  *] Initializing tunables: * (glob)
  *] Setting up derived data command for repo repo (glob)
  *] Completed derived data command setup for repo repo (glob)
  *] Initiating derived data command execution for repo repo* (glob)
  * using repo "repo" repoid RepositoryId(0)* (glob)
  * changeset resolved as: * (glob)
  *] derive fsnodes for c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd* (glob)
  * derived fsnodes in * (glob)
  *] Finished derived data command execution for repo repo* (glob)
  $ backfill_derived_data single c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd --all-types 2>&1 | grep 'derived .* in' | wc -l
  9
