# Copyright (c) Meta Platforms, Inc. and affiliates.
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
  $ mononoke_newadmin skiplist -R repo -k skiplist_4 build --exponent 2
  *] creating a skiplist from scratch (glob)
  *] built 5 skiplist nodes (glob)

enable some more derived data types for normal usage and backfilling
  $ SKIPLIST_INDEX_BLOBSTORE_KEY=skiplist_4 \
  >   ENABLED_DERIVED_DATA='["hgchangesets", "filenodes", "unodes", "fsnodes"]' \
  >   setup_mononoke_config
  $ cd "$TESTTMP"
  $ cat >> mononoke-config/repos/repo/server.toml <<CONFIG
  > [derived_data_config.available_configs.backfilling]
  > types=["blame", "skeleton_manifests", "unodes"]
  > CONFIG

start the tailer with tailing and backfilling some different types
normally the tailer runs forever, but for this test we will make it
stop when it becomes idle.
  $ backfill_derived_data tail --stop-on-idle --backfill --batched --parallel --sliced --slice-size=4 &> /dev/null

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
