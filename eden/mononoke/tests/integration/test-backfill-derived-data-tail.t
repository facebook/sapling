# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ ENABLED_DERIVED_DATA='["hgchangesets", "filenodes"]' setup_common_config
  $ cd "$TESTTMP"
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ drawdag <<EOS
  > M
  > |
  > L
  > |
  > K
  > |
  > J Q
  > | |
  > I P
  > | |
  > H O
  > | |
  > G N
  > |/
  > F
  > |
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg bookmark main -r $M
  $ hg bookmark other -r $Q
  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo

build the skiplist that will be used to slice the repository
  $ mononoke_admin skiplist build skiplist_4 --exponent 2
  *] using repo "repo" repoid RepositoryId(0) (glob)
  *] creating a skiplist from scratch (glob)
  *] build 17 skiplist nodes (glob)

enable some more derived data types for normal usage and backfilling
  $ SKIPLIST_INDEX_BLOBSTORE_KEY=skiplist_4 \
  >   ENABLED_DERIVED_DATA='["hgchangesets", "filenodes", "unodes", "fsnodes"]' \
  >   setup_mononoke_config
  $ cd "$TESTTMP"
  $ cat >> mononoke-config/repos/repo/server.toml <<CONFIG
  > [derived_data_config.backfilling]
  > types=["blame", "skeleton_manifests"]
  > CONFIG

start the tailer with tailing and backfilling some different types
normally the tailer runs forever, but for this test we will make it
stop when it becomes idle.
  $ backfill_derived_data tail --stop-on-idle --backfill --batched --parallel --sliced --slice-size=4
  *] using repo "repo" repoid RepositoryId(0) (glob)
  *] tailing derived data: {"filenodes", "fsnodes", "hgchangesets", "unodes"} (glob)
  *] backfilling derived data: {"blame", "filenodes", "fsnodes", "hgchangesets", "skeleton_manifests", "unodes"} (glob)
  *] Fetching and initializing skiplist (glob)
  *] cmap size 17, parent nodecount 0, skip nodecount 16, maxsedgelen 1, maxpedgelen 0 (glob)
  *] Built skiplist (glob)
  *] using batched deriver (glob)
  *] found changesets: 17 * (glob)
  *] deriving data 34 (glob)
  *] count:17 time:* start:* end:* (glob)
  *] count:17 time:* start:* end:* (glob)
  *] tail stopping due to --stop-on-idle (glob)
  *] Adding slice starting at generation 12 with 1 heads (1 slices queued) (glob)
  *] Adding slice starting at generation 8 with 2 heads (0 slices queued) (glob)
  *] Adding slice starting at generation 4 with 2 heads (0 slices queued) (glob)
  *] Adding slice starting at generation 0 with 2 heads (0 slices queued) (glob)
  *] Repository sliced into 4 slices requiring derivation (glob)
  *] Deriving slice 0 (1/4) with 2 heads (glob)
  *] found changesets: 3 * (glob)
  *] deriving data 4 (glob)
  *] count:2 time:* start:* end:* (glob)
  *] count:2 time:* start:* end:* (glob)
  *] Deriving slice 4 (2/4) with 2 heads (glob)
  *] found changesets: 5 * (glob)
  *] deriving data 8 (glob)
  *] count:4 time:* start:* end:* (glob)
  *] count:4 time:* start:* end:* (glob)
  *] Deriving slice 8 (3/4) with 2 heads (glob)
  *] found changesets: 7 * (glob)
  *] deriving data 14 (glob)
  *] count:7 time:* start:* end:* (glob)
  *] count:7 time:* start:* end:* (glob)
  *] Deriving slice 12 (4/4) with 1 heads (glob)
  *] found changesets: 4 * (glob)
  *] deriving data 8 (glob)
  *] count:4 time:* start:* end:* (glob)
  *] count:4 time:* start:* end:* (glob)
  *] backfill stopping (glob)

  $ mononoke_admin --log-level ERROR derived-data exists fsnodes main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ mononoke_admin --log-level ERROR derived-data exists fsnodes other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_admin --log-level ERROR derived-data exists unodes main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ mononoke_admin --log-level ERROR derived-data exists unodes other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_admin --log-level ERROR derived-data exists --backfill blame main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ mononoke_admin --log-level ERROR derived-data exists --backfill blame other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_admin --log-level ERROR derived-data exists --backfill skeleton_manifests other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_admin --log-level ERROR derived-data exists --backfill skeleton_manifests main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
