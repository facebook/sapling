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
  *] Initializing JustKnobs: * (glob)
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
  *] derive exactly fsnodes batch from 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec to 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec* (glob)
  * 1/1 * (glob)
  *] Finished derived data command execution for repo repo* (glob)
  $ hg log -r "min(all())" -T '{node}'
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 (no-eol)
  $ mononoke_newadmin derived-data -R repo exists -T "$DERIVED_DATA_TYPE" --hg-id 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  Derived: 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec
  $ backfill_derived_data backfill --prefetched-commits-path "$TESTTMP/prefetched_commits" "$DERIVED_DATA_TYPE" --skip-changesets 1
  *] enabled stdlog with level: Error (set RUST_LOG to configure) (glob)
  *] Initializing tunables: * (glob)
  *] Initializing JustKnobs: * (glob)
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
  *] derive exactly fsnodes batch from 459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66 to c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd* (glob)
  * 2/2 * (glob)
  *] Finished derived data command execution for repo repo* (glob)

  $ mononoke_newadmin derived-data -R repo exists -T "$DERIVED_DATA_TYPE" -B master_bookmark
  Derived: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd

  $ backfill_derived_data single c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd "$DERIVED_DATA_TYPE"
  *] enabled stdlog with level: Error (set RUST_LOG to configure) (glob)
  *] Initializing tunables: * (glob)
  *] Initializing JustKnobs: * (glob)
  *] Setting up derived data command for repo repo (glob)
  *] Completed derived data command setup for repo repo (glob)
  *] Initiating derived data command execution for repo repo* (glob)
  * using repo "repo" repoid RepositoryId(0)* (glob)
  * changeset resolved as: * (glob)
  *] derive fsnodes for c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd* (glob)
  * derived fsnodes in * (glob)
  *] Finished derived data command execution for repo repo* (glob)
  $ backfill_derived_data single c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd --all-types 2>&1 | grep 'derived .* in' | wc -l
  13

  $ testtool_drawdag -R repo <<EOF
  > C-D-E-F-G
  >      \
  >       H
  > # exists: C c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  > EOF
  C=c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  D=81933c8d8759698bb4052d063a6d7a4ec31a7b31416c24bc302ff99baae915b6
  E=0b22a371e68006839d9f5ff1286c7fffc72e1db96737a75898ededde45a95032
  F=1285e6a06283ff3cfe6e20035e3d730d8c8fb8c19213b1213e5403982c423d00
  G=1c0cf47cdb1ea6e4aa571543bf5047bf8354e354b3b20fa48c16c659df54f26a
  H=ebc9ac5205f2a188f62a5fa43ba092ba4e51744992317aea5b7ee64657c21110

  $ backfill_derived_data backfill-all --parallel --batch-size=10 --changeset $G 2>&1 | grep 'found changesets:'
  * found changesets: 4 * (glob)
