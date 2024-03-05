# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ ENABLED_DERIVED_DATA='["hgchangesets", "filenodes", "unodes", "fsnodes", "blame"]' setup_common_config
  $ cd "$TESTTMP"
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ drawdag <<EOS
  > A-B-C-D-E-F-G-H-I-J-K-L-M
  > EOS
  $ hg bookmark main -r $M
  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo

#enable some more derived data types for normal usage and backfilling
#  >   setup_mononoke_config
#  $ . "${TEST_FIXTURES}/library.sh"


backfill derived data
  $ quiet mononoke_newadmin dump-changesets -R repo --out-filename "$TESTTMP/prefetched_commits" fetch-public

  $ backfill_derived_data backfill --prefetched-commits-path "$TESTTMP/prefetched_commits" blame
  * enabled stdlog with level: Error (set RUST_LOG to configure) (glob)
  * Initializing JustKnobs: * (glob)
  * Setting up derived data command for repo repo (glob)
  * Completed derived data command setup for repo repo (glob)
  * Initiating derived data command execution for repo repo* (glob)
  * using repo "repo" repoid RepositoryId(0)* (glob)
  * Reloading redacted config from configerator (glob)
  * Initializing CfgrLiveCommitSyncConfig, repo: repo (glob)
  * Initialized PushRedirect configerator config, repo: repo (glob)
  * Initialized all commit sync versions configerator config, repo: repo (glob)
  * Done initializing CfgrLiveCommitSyncConfig, repo: repo (glob)
  * reading all changesets for: RepositoryId(0)* (glob)
  * starting deriving data for 13 changesets* (glob)
  * starting batch of 13 from 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec* (glob)
  * warmup of 13 changesets complete* (glob)
  * derive exactly unodes batch from 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec to 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7, repo: repo (glob)
  * derive unode batch at 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec (stack of 13 from batch of 13), repo: repo (glob)
  * derive exactly blame batch from 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec to 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7, repo: repo (glob)
  * derive blame batch at 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec (stack of 13 from batch of 13), repo: repo (glob)
  * 13/13 (13 in *) estimate:* speed:*s overall_speed:*, repo: repo (glob)
  * Finished derived data command execution for repo repo, repo: repo (glob)

  $ mononoke_newadmin derived-data -R repo exists -T blame -B main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
