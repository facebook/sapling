# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ REPOID=0 REPONAME=large setup_common_config blob_files
  $ REPOID=1 REPONAME=small setup_common_config blob_files

  $ cat > "$COMMIT_SYNC_CONF/all" << EOF
  > {
  > "repos": {
  >   "large": {
  >     "versions": [
  >       {
  >         "common_pushrebase_bookmarks": ["main"],
  >         "large_repo_id": 0,
  >         "small_repos": [
  >           {
  >             "repoid": 1,
  >             "default_action": "prepend_prefix",
  >             "default_prefix": "smallrepofolder"
  >           }
  >         ],
  >         "version_name": "test_version"
  >       }
  >     ],
  >     "common": {
  >       "common_pushrebase_bookmarks": ["main"],
  >       "large_repo_id": 0,
  >       "small_repos": {
  >         1: {
  >           "bookmark_prefix": "small_repo_prefix/",
  >           "common_pushrebase_bookmarks_map": { "main": "heads/main" }
  >         }
  >       }
  >     }
  >   }
  > }
  > }
  > EOF

  $ testtool_drawdag -R large << EOF
  > LA-LB-LC
  >   \LD
  > # bookmark: LA main
  > # bookmark: LA common_bookmark
  > EOF
  LA=b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a
  LB=b4534a5a2c4570a7f76b86de82927a8b35da5bc69f7618fe6724f09fa183ad25
  LC=207c0c64826e67a9fde5993ff2789ed838376b082656f49ce239536193a832bd
  LD=55e6107d94bc705f7d97f90f13ba45aaaf505b36eba1f30c7b9361399725a4a8

  $ testtool_drawdag -R small << EOF
  > SA-SB-SC
  >   \SD-SE
  > # bookmark: SA heads/main
  > # bookmark: SA heads/common_bookmark
  > # bookmark: SA heads/other_bookmark
  > EOF
  SA=7c5a873e5729acecbe37ac89b3f7cbc4292cd8cbcff60f39126ed74d9f55e05e
  SB=dd912eedd1899b2403fc507d74bec70bda5f4a035cd9851478847bc2b35dfa3f
  SC=9c27228ce4dda0e66c126c4560521707a6fc3e48d79d471bede547a76987d3be
  SD=dff952c4a2e8b8933ccb668b2d67697de1a68b1f0e1480574bef4ce1240fac29
  SE=4edb9d3bbdc18205682d544efd6fc19bd92a932a7e087330354e3da341738ec9

-- insert sync mapping entry for SA and DA which are equivalent
  $ add_synced_commit_mapping_entry 1 $SA 0 $LA test_version
  $ mononoke_newadmin bookmarks --repo-name large list
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a common_bookmark
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a main

-- start mononoke
  $ start_and_wait_for_mononoke_server
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_1', 0)";

-- sync once
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once
  * using repo "small" repoid RepositoryId(1) (glob)
  * using repo "large" repoid RepositoryId(0) (glob)
  * using repo "small" repoid RepositoryId(1) (glob)
  * using repo "large" repoid RepositoryId(0) (glob)
  * queue size is 3 (glob)
  * processing log entry #1 (glob)
  * 0 unsynced ancestors of 7c5a873e5729acecbe37ac89b3f7cbc4292cd8cbcff60f39126ed74d9f55e05e (glob)
  * successful sync bookmark update log #1 (glob)
  * processing log entry #2 (glob)
  * 0 unsynced ancestors of 7c5a873e5729acecbe37ac89b3f7cbc4292cd8cbcff60f39126ed74d9f55e05e (glob)
  * successful sync bookmark update log #2 (glob)
  * processing log entry #3 (glob)
  * 0 unsynced ancestors of 7c5a873e5729acecbe37ac89b3f7cbc4292cd8cbcff60f39126ed74d9f55e05e (glob)
  * successful sync bookmark update log #3 (glob)

  $ mononoke_newadmin bookmarks --repo-name large list
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a common_bookmark
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a main
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a small_repo_prefix/heads/common_bookmark
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a small_repo_prefix/heads/main
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a small_repo_prefix/heads/other_bookmark

-- move the bookmarks
  $ testtool_drawdag -R large << EOF
  > LA-LB-LC
  >   \LD
  > # exists: LC $LC
  > # exists: LD $LD
  > # bookmark: LC main
  > # bookmark: LD common_bookmark
  > EOF
  LA=b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a
  LB=b4534a5a2c4570a7f76b86de82927a8b35da5bc69f7618fe6724f09fa183ad25
  LC=207c0c64826e67a9fde5993ff2789ed838376b082656f49ce239536193a832bd
  LD=55e6107d94bc705f7d97f90f13ba45aaaf505b36eba1f30c7b9361399725a4a8
  $ testtool_drawdag -R small << EOF
  > SA-SB-SC
  >   \SD-SE
  > # exists: SB $SB
  > # exists: SC $SC
  > # exists: SD $SD
  > # exists: SE $SE
  > # bookmark: SC heads/main
  > # bookmark: SB tags/release_b
  > # bookmark: SD heads/common_bookmark
  > # bookmark: SE heads/other_bookmark
  > EOF
  SA=7c5a873e5729acecbe37ac89b3f7cbc4292cd8cbcff60f39126ed74d9f55e05e
  SB=dd912eedd1899b2403fc507d74bec70bda5f4a035cd9851478847bc2b35dfa3f
  SC=9c27228ce4dda0e66c126c4560521707a6fc3e48d79d471bede547a76987d3be
  SD=dff952c4a2e8b8933ccb668b2d67697de1a68b1f0e1480574bef4ce1240fac29
  SE=4edb9d3bbdc18205682d544efd6fc19bd92a932a7e087330354e3da341738ec9

-- sync again
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once
  * using repo "small" repoid RepositoryId(1) (glob)
  * using repo "large" repoid RepositoryId(0) (glob)
  * using repo "small" repoid RepositoryId(1) (glob)
  * using repo "large" repoid RepositoryId(0) (glob)
  * queue size is 4 (glob)
  * processing log entry #4 (glob)
  * 1 unsynced ancestors of dd912eedd1899b2403fc507d74bec70bda5f4a035cd9851478847bc2b35dfa3f (glob)
  * syncing dd912eedd1899b2403fc507d74bec70bda5f4a035cd9851478847bc2b35dfa3f (glob)
  * changeset dd912eedd1899b2403fc507d74bec70bda5f4a035cd9851478847bc2b35dfa3f synced as 540414777d82df622b2762dc79dfba2d92f994481105e34d47274c4742dbedb0 * (glob)
  * successful sync bookmark update log #4 (glob)
  * processing log entry #5 (glob)
  * Skipped syncing log entry #5 because no mapping version found. Is it a new root commit in the repo? (glob)
  * processing log entry #6 (glob)
  * 1 unsynced ancestors of 9c27228ce4dda0e66c126c4560521707a6fc3e48d79d471bede547a76987d3be (glob)
  * syncing 9c27228ce4dda0e66c126c4560521707a6fc3e48d79d471bede547a76987d3be (glob)
  * changeset 9c27228ce4dda0e66c126c4560521707a6fc3e48d79d471bede547a76987d3be synced as fbdcc859102f75ca8d2d5b66313e6e32f3ec6a0e35e130350053f3ac097eb705 * (glob)
  * successful sync bookmark update log #6 (glob)
  * processing log entry #7 (glob)
  * Skipped syncing log entry #7 because no mapping version found. Is it a new root commit in the repo? (glob)

-- check the state of bookmarks in the large repo
-- Once we support different names for the same bookmark between small and large repo,
--  * main shouldn't be 207c0c6 as that is the same as LC, indicating that it wasn't modified by importing SC
--  * small_repo_prefix/heads/main should not exist as that bookmark should be push-rebased onto main
  $ mononoke_newadmin bookmarks --repo-name large list
  55e6107d94bc705f7d97f90f13ba45aaaf505b36eba1f30c7b9361399725a4a8 common_bookmark
  207c0c64826e67a9fde5993ff2789ed838376b082656f49ce239536193a832bd main
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a small_repo_prefix/heads/common_bookmark
  fbdcc859102f75ca8d2d5b66313e6e32f3ec6a0e35e130350053f3ac097eb705 small_repo_prefix/heads/main
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a small_repo_prefix/heads/other_bookmark
  540414777d82df622b2762dc79dfba2d92f994481105e34d47274c4742dbedb0 small_repo_prefix/tags/release_b

-- check the mutable counters
$ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters where name = 'xreposync_from_1'";

