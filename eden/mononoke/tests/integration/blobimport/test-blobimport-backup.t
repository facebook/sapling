# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_pre_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $

  $ setup_mononoke_config
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo --commits-limit 2
  $ blobimport --log repo-hg/.hg repo --find-already-imported-rev-only
  * using repo "repo" repoid RepositoryId(0) (glob)
  * latest imported revision 1 (glob)
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters";
  0|highest-imported-gen-num|2

  $ REPONAME=backup REPOID=2 setup_mononoke_config
  $ cd $TESTTMP/repo-hg

  $ hg up master_bookmark
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)

# Check content_id for file B
  $ mononoke_newadmin filestore -R repo store B
  Wrote 55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f (1 bytes)
# Upload C as it wasn't imported
  $ mononoke_newadmin filestore -R repo store C
  Wrote 896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d (1 bytes)
  $ cd $TESTTMP

  $ cat > bonsai_file <<EOF
  > {
  >   "parents": [
  >     "459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66"
  >   ],
  >   "author": "test",
  >   "author_date": "1970-01-01T00:00:00+00:00",
  >   "committer": null,
  >   "committer_date": null,
  >   "message": "C",
  >   "hg_extra": {},
  >   "file_changes": {
  >     "C": {
  >       "Change": {
  >         "inner": {
  >           "content_id": "896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d",
  >           "file_type": "Regular",
  >           "size": 1
  >         },
  >         "copy_from": null
  >       }
  >     },
  >     "B": {
  >       "Change": {
  >         "inner": {
  >           "content_id": "55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f",
  >           "file_type": "Regular",
  >           "size": 1
  >         },
  >         "copy_from": null
  >       }
  >     }
  >   }
  > }
  > EOF
  $ mononoke_testtool create-bonsai -R repo bonsai_file
  Created bonsai changeset 4b71c845e8783e58fce825fa80254840eba291d323a5d69218ad927fc801153c for Hg changeset 26805aba1e600a82e93661149f2313866a221a7b
  $ mononoke_newadmin bookmarks -R repo set master_bookmark 26805aba1e600a82e93661149f2313866a221a7b
  Creating publishing bookmark master_bookmark at 4b71c845e8783e58fce825fa80254840eba291d323a5d69218ad927fc801153c
  $ mononoke_newadmin bookmarks -R repo list
  4b71c845e8783e58fce825fa80254840eba291d323a5d69218ad927fc801153c master_bookmark

  $ REPOID=2 blobimport repo-hg/.hg backup --backup-from-repo-name repo
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters";
  0|highest-imported-gen-num|2
  2|highest-imported-gen-num|3
  $ mononoke_newadmin bookmarks --repo-id=2 list
  4b71c845e8783e58fce825fa80254840eba291d323a5d69218ad927fc801153c master_bookmark
