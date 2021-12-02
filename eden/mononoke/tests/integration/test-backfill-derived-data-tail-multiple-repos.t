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

Helpers to setup skiplist for repos and
enable some more derived data types for normal usage and backfilling
  $ function prod_setup_skiplist() {
  >   mononoke_admin skiplist build skiplist_4 --exponent 2
  >   SKIPLIST_INDEX_BLOBSTORE_KEY=skiplist_4 \
  >     ENABLED_DERIVED_DATA='["hgchangesets", "filenodes", "unodes", "fsnodes"]' \
  >     setup_mononoke_config
  >   cd "$TESTTMP"
  >   cat >> mononoke-config/repos/repo/server.toml <<CONFIG
  > [derived_data_config.available_configs.backfilling]
  > types=["blame", "skeleton_manifests", "unodes"]
  > CONFIG
  > }

  $ function backup_setup_skiplist() {
  >   REPOID=1 mononoke_admin skiplist build skiplist_4 --exponent 2
  >   REPOID=1 REPONAME=backup SKIPLIST_INDEX_BLOBSTORE_KEY=skiplist_4 \
  >     ENABLED_DERIVED_DATA='["hgchangesets", "filenodes", "unodes", "fsnodes"]' \
  >     setup_mononoke_config
  >   cd "$TESTTMP"
  >   cat >> mononoke-config/repos/backup/server.toml <<CONFIG
  > [derived_data_config.available_configs.backfilling]
  > types=["blame", "skeleton_manifests", "unodes"]
  > CONFIG
  > }

build the skiplist that will be used to slice the repository
  $ prod_setup_skiplist
  *] using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  *] creating a skiplist from scratch (glob)
  *] build 5 skiplist nodes (glob)

  $ cd "$TESTTMP"
  $ REPOID=1 REPONAME=backup setup_common_config
  $ cd "$TESTTMP"
  $ REPOID=1 blobimport repo-hg/.hg backup --backup-from-repo-name repo

build the skiplist that will be used to slice the repository
  $ backup_setup_skiplist
  *] using repo "backup" repoid RepositoryId(1) (glob)
  *Reloading redacted config from configerator* (glob)
  *] creating a skiplist from scratch (glob)
  *] build 5 skiplist nodes (glob)

start the tailer with tailing and backfilling some different types
normally the tailer runs forever, but for this test we will make it
stop when it becomes idle.
  $ REPOS="--repo-id=0:--repo-id=1" backfill_derived_data_multiple_repos tail --stop-on-idle --backfill --batched --parallel --sliced --slice-size=4 &>/dev/null

Helpers for check derived data for commit:
  $ function prod_derived_data_exists() {
  >   mononoke_admin --log-level ERROR derived-data exists "$@"
  > }

  $ function backup_derived_data_exists() {
  >   REPOID=1 mononoke_admin --log-level ERROR derived-data exists "$@"
  > }

  $ prod_derived_data_exists fsnodes main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ prod_derived_data_exists fsnodes other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ prod_derived_data_exists unodes main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ prod_derived_data_exists unodes other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ prod_derived_data_exists --backfill blame main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ prod_derived_data_exists --backfill blame other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ prod_derived_data_exists --backfill skeleton_manifests other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ prod_derived_data_exists --backfill skeleton_manifests main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7


  $ backup_derived_data_exists fsnodes main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ backup_derived_data_exists fsnodes other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ backup_derived_data_exists unodes main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ backup_derived_data_exists unodes other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ backup_derived_data_exists --backfill blame main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ backup_derived_data_exists --backfill blame other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ backup_derived_data_exists --backfill skeleton_manifests other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ backup_derived_data_exists --backfill skeleton_manifests main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
