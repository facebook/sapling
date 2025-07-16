# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ export CAS_STORE_PATH="$TESTTMP"

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo
  $ drawdag << EOS
  > D # D/bar = zero\nuno\ntwo\n
  > |
  > C # C/bar = zero\none\ntwo\n (renamed from foo)
  > |
  > B # B/foo = one\ntwo\n
  > |
  > A # A/foo = one\n
  > EOS

  $ hg goto A -q
  $ hg push -r . --to master_bookmark -q --create

  $ hg goto B -q
  $ hg push -r . --to master_bookmark -q

  $ hg goto C -q
  $ hg push -r . --to master_bookmark -q

  $ hg goto D -q
  $ hg push -r . --to master_bookmark -q

Check that new entry was added to the sync database. 4 pushes
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) from bookmarks_update_log";
  4

Sync all bookmarks moves
  $ mononoke_cas_sync repo 0
  [INFO] [execute{repo=repo}] Initiating mononoke RE CAS sync command execution
  [INFO] [execute{repo=repo}] using repo "repo" repoid RepositoryId(0)
  [INFO] [execute{repo=repo}] syncing log entries [1, 2, 3, 4] ...
  [INFO] [execute{repo=repo}] log entry BookmarkUpdateLogEntry * is a creation of bookmark (glob)
  [INFO] [execute{repo=repo}] log entries [1, 2, 3, 4] synced (4 commits uploaded, upload stats: uploaded digests: 12, already present digests: 0, uploaded bytes: 2.6 KiB, the largest uploaded blob: 862 B), took overall * sec (glob)
  [INFO] [execute{repo=repo}] queue size after processing: 0
  [INFO] [execute{repo=repo}] successful sync of entries [1, 2, 3, 4]
  [INFO] [execute{repo=repo}] Finished mononoke RE CAS sync command execution for repo repo

Validate that the whole working copy for the top commit D is already present in CAS, nothing should be uploaded if incremental sync is correct.
All trees and blobs should be present!
  $ mononoke_admin cas-store --repo-name repo upload --full -i $D
  [INFO] Upload completed. Upload stats: uploaded digests: 0, already present digests: 6, uploaded bytes: 0 B, the largest uploaded blob: 0 B

Validate the same for a middle commit B
  $ mononoke_admin cas-store --repo-name repo upload --full -i $B
  [INFO] Upload completed. Upload stats: uploaded digests: 0, already present digests: 4, uploaded bytes: 0 B, the largest uploaded blob: 0 B

  $ mononoke_admin derived-data --repo-name repo fetch -i $B -T hg_augmented_manifests
  Derived: c5ae951c61da1af20ce42287cca9f1e2a10e8e16e5fe6fff96221529de1391c7 -> RootHgAugmentedManifestId(HgAugmentedManifestId(HgNodeHash(Sha1(3071e3203526cb812525ec7838669975e6113567))))

  $ mononoke_admin cas-store --repo-name repo tree-info -i 3071e3203526cb812525ec7838669975e6113567 -c
  CAS digest: 8f8e91f9bca60b3b3bf51c6b8cff9042b7f28fc495e3f97d6abc2fca2244c386:553
  A -> File CAS digest: 5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda:1 HgId: 005d992c5dcf32993668f7cede29d296c494a5d9
  B -> File CAS digest: 5667f2421ac250c4bb9af657b5ead3cdbd940bfbc350b2bfee47454643832b48:1 HgId: 35e7525ce3a48913275d7061dd9a867ffef1e34d
  foo -> File CAS digest: c0dc9fb94012c02a11f30c7f2533d6e8ab55a3b42726c00c02cd4fa1c1eb920c:8 HgId: e69018796d5c4e6314c9ee3c7131abc3349b5dba

  $ mononoke_admin cas-store --repo-name repo file-info -i 005d992c5dcf32993668f7cede29d296c494a5d9
  CAS digest: 5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda:1
