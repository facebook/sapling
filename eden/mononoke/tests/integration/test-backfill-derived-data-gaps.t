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
  > H
  > |
  > G
  > |
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
  $ hg bookmark main -r $H
  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo

enable some more derived data types for normal usage and backfilling
add a mapping key prefix to skeleton manifests to test these work
  $ ENABLED_DERIVED_DATA='["hgchangesets", "filenodes", "unodes", "fsnodes"]' \
  >   setup_mononoke_config
  $ cd "$TESTTMP"
  $ cat >> mononoke-config/repos/repo/server.toml <<CONFIG
  > [derived_data_config.available_configs.backfilling]
  > types=["blame", "skeleton_manifests", "unodes"]
  > mapping_key_prefixes={"skeleton_manifests"="xyz."}
  > CONFIG

Backfill all enabled data types. That command uses the same logic as tailer does
but doesn't run forever.
  $ backfill_derived_data backfill-all --parallel --batch-size=10 --gap-size=3 &>/dev/null

Heads should all be derived
  $ mononoke_newadmin derived-data -R repo exists -T fsnodes -B main
  Derived: 8ea58cff262ad56732037fb42189d6262dacdaf8032c18ddebcb6b5b310d1298
  $ mononoke_newadmin derived-data -R repo exists -T unodes -B main
  Derived: 8ea58cff262ad56732037fb42189d6262dacdaf8032c18ddebcb6b5b310d1298
  $ mononoke_newadmin derived-data -R repo exists --backfill -T blame -B main
  Derived: 8ea58cff262ad56732037fb42189d6262dacdaf8032c18ddebcb6b5b310d1298
  $ mononoke_newadmin derived-data -R repo exists --backfill -T skeleton_manifests -B main
  Derived: 8ea58cff262ad56732037fb42189d6262dacdaf8032c18ddebcb6b5b310d1298

Commits at the gap boundaries should be derived
  $ mononoke_newadmin derived-data -R repo exists -T fsnodes --hg-id $C
  Derived: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  $ mononoke_newadmin derived-data -R repo exists -T fsnodes --hg-id $F
  Derived: 3eb8abc0587595debd43ac6f36b0e6fbb6404c3bb810015f0224c2653ee6b195
  $ mononoke_newadmin derived-data -R repo exists -T unodes --hg-id $C
  Derived: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  $ mononoke_newadmin derived-data -R repo exists -T unodes --hg-id $F
  Derived: 3eb8abc0587595debd43ac6f36b0e6fbb6404c3bb810015f0224c2653ee6b195
  $ mononoke_newadmin derived-data -R repo exists --backfill -T blame --hg-id $C
  Derived: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  $ mononoke_newadmin derived-data -R repo exists --backfill -T blame --hg-id $F
  Derived: 3eb8abc0587595debd43ac6f36b0e6fbb6404c3bb810015f0224c2653ee6b195
  $ mononoke_newadmin derived-data -R repo exists --backfill -T skeleton_manifests --hg-id $C
  Derived: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  $ mononoke_newadmin derived-data -R repo exists --backfill -T skeleton_manifests --hg-id $F
  Derived: 3eb8abc0587595debd43ac6f36b0e6fbb6404c3bb810015f0224c2653ee6b195

Other commits should not be derived, for types where gaps are supported
  $ mononoke_newadmin derived-data -R repo exists -T fsnodes --hg-id $B
  Not Derived: 459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66
  $ mononoke_newadmin derived-data -R repo exists -T fsnodes --hg-id $G
  Not Derived: da6d6ff8b30c472a08a1252ccb81dd4a0f9f3212af2e631a6a9a6b78ad78f6f4
  $ mononoke_newadmin derived-data -R repo exists --backfill -T skeleton_manifests --hg-id $B
  Not Derived: 459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66
  $ mononoke_newadmin derived-data -R repo exists --backfill -T skeleton_manifests --hg-id $G
  Not Derived: da6d6ff8b30c472a08a1252ccb81dd4a0f9f3212af2e631a6a9a6b78ad78f6f4

They should be derived for types that don't support gaps
  $ mononoke_newadmin derived-data -R repo exists -T unodes --hg-id $B
  Derived: 459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66
  $ mononoke_newadmin derived-data -R repo exists -T unodes --hg-id $G
  Derived: da6d6ff8b30c472a08a1252ccb81dd4a0f9f3212af2e631a6a9a6b78ad78f6f4
  $ mononoke_newadmin derived-data -R repo exists --backfill -T blame --hg-id $B
  Derived: 459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66
  $ mononoke_newadmin derived-data -R repo exists --backfill -T blame --hg-id $G
  Derived: da6d6ff8b30c472a08a1252ccb81dd4a0f9f3212af2e631a6a9a6b78ad78f6f4

Skeleton manifest blob keys should have their prefix
  $ mononoke_newadmin --log-level ERROR blobstore -R repo fetch derived_root_skeletonmanifest.xyz.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd --output $TESTTMP/skmf-root
  Key: derived_root_skeletonmanifest.xyz.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  Ctime: * (glob)
  Size: 32
  
