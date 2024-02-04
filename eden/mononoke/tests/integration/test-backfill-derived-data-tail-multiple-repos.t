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

Helpers to enable some more derived data types for normal usage and backfilling
  $ function prod_setup() {
  >   ENABLED_DERIVED_DATA='["hgchangesets", "filenodes", "unodes", "fsnodes"]' \
  >     setup_mononoke_config
  >   cd "$TESTTMP"
  >   cat >> mononoke-config/repos/repo/server.toml <<CONFIG
  > [derived_data_config.available_configs.backfilling]
  > types=["blame", "skeleton_manifests", "unodes"]
  > CONFIG
  > }

  $ function backup_setup() {
  >   REPOID=1 REPONAME=backup \
  >     ENABLED_DERIVED_DATA='["hgchangesets", "filenodes", "unodes", "fsnodes"]' \
  >     setup_mononoke_config
  >   cd "$TESTTMP"
  >   cat >> mononoke-config/repos/backup/server.toml <<CONFIG
  > [derived_data_config.available_configs.backfilling]
  > types=["blame", "skeleton_manifests", "unodes"]
  > CONFIG
  > }

  $ prod_setup

  $ cd "$TESTTMP"
  $ REPOID=1 REPONAME=backup setup_common_config
  $ cd "$TESTTMP"
  $ REPOID=1 blobimport repo-hg/.hg backup --backup-from-repo-name repo

  $ backup_setup

start the tailer with tailing and backfilling some different types
normally the tailer runs forever, but for this test we will make it
stop when it becomes idle.
  $ REPOS="--repo-id=0:--repo-id=1" backfill_derived_data_multiple_repos tail --stop-on-idle --backfill --batched --parallel --sliced --slice-size=4 &>/dev/null

  $ mononoke_newadmin derived-data -R repo exists -T fsnodes -B main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ mononoke_newadmin derived-data -R repo exists -T fsnodes -B other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_newadmin derived-data -R repo exists -T unodes -B main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ mononoke_newadmin derived-data -R repo exists -T unodes -B other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_newadmin derived-data -R repo exists --backfill -T blame -B main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ mononoke_newadmin derived-data -R repo exists --backfill -T blame -B other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_newadmin derived-data -R repo exists --backfill -T skeleton_manifests -B other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_newadmin derived-data -R repo exists --backfill -T skeleton_manifests -B main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7


  $ mononoke_newadmin derived-data -R backup exists -T fsnodes -B main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ mononoke_newadmin derived-data -R backup exists -T fsnodes -B other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_newadmin derived-data -R backup exists -T unodes -B main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ mononoke_newadmin derived-data -R backup exists -T unodes -B other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_newadmin derived-data -R backup exists --backfill -T blame -B main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
  $ mononoke_newadmin derived-data -R backup exists --backfill -T blame -B other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_newadmin derived-data -R backup exists --backfill -T skeleton_manifests -B other
  Derived: 39f5c6f537a8c1157a7f92a39bb036f58c03269fbe244cccaf6489fd26813467
  $ mononoke_newadmin derived-data -R backup exists --backfill -T skeleton_manifests -B main
  Derived: 544c0991ef12b0621aa901dd64ef65f539246646faa940171850f5e11c84cda7
