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
  >             "default_prefix": "smallrepofolder",
  >             "direction": "small_to_large"
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
  >   \ 
  >    LD
  > # bookmark: LA main
  > # bookmark: LA common_bookmark
  > EOF
  LA=b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a
  LB=b4534a5a2c4570a7f76b86de82927a8b35da5bc69f7618fe6724f09fa183ad25
  LC=207c0c64826e67a9fde5993ff2789ed838376b082656f49ce239536193a832bd
  LD=d52189c4c92bc5d3c99269e794afb72c31caf9ed4eba39abbb3c2739f010096d

  $ testtool_drawdag -R small << EOF
  > SA-SB-SC
  >   \ 
  >    SD-SE
  > # bookmark: SA heads/main
  > # bookmark: SA heads/common_bookmark
  > # bookmark: SA heads/other_bookmark
  > EOF
  SA=7c5a873e5729acecbe37ac89b3f7cbc4292cd8cbcff60f39126ed74d9f55e05e
  SB=dd912eedd1899b2403fc507d74bec70bda5f4a035cd9851478847bc2b35dfa3f
  SC=9c27228ce4dda0e66c126c4560521707a6fc3e48d79d471bede547a76987d3be
  SD=2e8a00bae279cc78af17418196b1d6e78730b82752d758c7306f3b281038e8a3
  SE=21fa4d2997f1f2e050b3911639e9643efcb90896b3551b0a9c6affc33c7ea708

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
  * Starting session with id * (glob)
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
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a small_repo_prefix/heads/other_bookmark

-- move the bookmarks
  $ testtool_drawdag -R large << EOF
  > LA-LB-LC
  >   \ 
  >    LD
  > # exists: LC $LC
  > # exists: LD $LD
  > # bookmark: LC main
  > # bookmark: LD common_bookmark
  > EOF
  LA=b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a
  LB=b4534a5a2c4570a7f76b86de82927a8b35da5bc69f7618fe6724f09fa183ad25
  LC=207c0c64826e67a9fde5993ff2789ed838376b082656f49ce239536193a832bd
  LD=d52189c4c92bc5d3c99269e794afb72c31caf9ed4eba39abbb3c2739f010096d
-- move heads/main in the small repo (the common pushrebase bookmark)
  $ testtool_drawdag -R small << EOF
  > SA-SB-SC
  >   \ 
  >    SD-SE
  > # exists: SA $SA
  > # exists: SB $SB
  > # exists: SC $SC
  > # exists: SD $SD
  > # exists: SE $SE
  > # bookmark: SC heads/main
  > EOF
  SA=7c5a873e5729acecbe37ac89b3f7cbc4292cd8cbcff60f39126ed74d9f55e05e
  SB=dd912eedd1899b2403fc507d74bec70bda5f4a035cd9851478847bc2b35dfa3f
  SC=9c27228ce4dda0e66c126c4560521707a6fc3e48d79d471bede547a76987d3be
  SD=2e8a00bae279cc78af17418196b1d6e78730b82752d758c7306f3b281038e8a3
  SE=21fa4d2997f1f2e050b3911639e9643efcb90896b3551b0a9c6affc33c7ea708

-- sync 
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once
  * Starting session with id * (glob)
  * queue size is 1 (glob)
  * processing log entry #4 (glob)
  * 2 unsynced ancestors of 9c27228ce4dda0e66c126c4560521707a6fc3e48d79d471bede547a76987d3be (glob)
  * syncing dd912eedd1899b2403fc507d74bec70bda5f4a035cd9851478847bc2b35dfa3f via pushrebase for main (glob)
  * changeset dd912eedd1899b2403fc507d74bec70bda5f4a035cd9851478847bc2b35dfa3f synced as ccca231a326f5060eebc66ed1f1ad6aaa1490f1d7faa40cc469e59bf5a4e1ee9 * (glob)
  * syncing 9c27228ce4dda0e66c126c4560521707a6fc3e48d79d471bede547a76987d3be via pushrebase for main (glob)
  * changeset 9c27228ce4dda0e66c126c4560521707a6fc3e48d79d471bede547a76987d3be synced as e3c1a924a6c56bf0dfbd0173601239a4776b6ed17075617270c27c80456e12fb * (glob)
  * successful sync bookmark update log #4 (glob)

  $ mononoke_newadmin bookmarks --repo-name large list
  d52189c4c92bc5d3c99269e794afb72c31caf9ed4eba39abbb3c2739f010096d common_bookmark
  e3c1a924a6c56bf0dfbd0173601239a4776b6ed17075617270c27c80456e12fb main
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a small_repo_prefix/heads/common_bookmark
  b32ac5e4bcae0f9e8a25327e30674cf4f81ade62b6fb6fdf4e561f5099ec396a small_repo_prefix/heads/other_bookmark

-- move other bookmarks
  $ testtool_drawdag -R small << EOF
  > SA-SB-SC
  >   \ 
  >    SD-SE
  > # exists: SA $SA
  > # exists: SB $SB
  > # exists: SC $SC
  > # exists: SD $SD
  > # exists: SE $SE
  > # bookmark: SB tags/release_b
  > # bookmark: SD heads/common_bookmark
  > # bookmark: SE heads/other_bookmark
  > EOF
  SA=7c5a873e5729acecbe37ac89b3f7cbc4292cd8cbcff60f39126ed74d9f55e05e
  SB=dd912eedd1899b2403fc507d74bec70bda5f4a035cd9851478847bc2b35dfa3f
  SC=9c27228ce4dda0e66c126c4560521707a6fc3e48d79d471bede547a76987d3be
  SD=2e8a00bae279cc78af17418196b1d6e78730b82752d758c7306f3b281038e8a3
  SE=21fa4d2997f1f2e050b3911639e9643efcb90896b3551b0a9c6affc33c7ea708

-- sync again
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once
  * Starting session with id * (glob)
  * queue size is 3 (glob)
  * processing log entry #5 (glob)
  * 0 unsynced ancestors of dd912eedd1899b2403fc507d74bec70bda5f4a035cd9851478847bc2b35dfa3f (glob)
  * successful sync bookmark update log #5 (glob)
  * processing log entry #6 (glob)
  * 1 unsynced ancestors of 2e8a00bae279cc78af17418196b1d6e78730b82752d758c7306f3b281038e8a3 (glob)
  * syncing 2e8a00bae279cc78af17418196b1d6e78730b82752d758c7306f3b281038e8a3 (glob)
  * changeset 2e8a00bae279cc78af17418196b1d6e78730b82752d758c7306f3b281038e8a3 synced as b149e7a688c2bac6a2f25dc4b846060f774967ae82eeac54cc1a7c148b215b27 * (glob)
  * successful sync bookmark update log #6 (glob)
  * processing log entry #7 (glob)
  * 1 unsynced ancestors of 21fa4d2997f1f2e050b3911639e9643efcb90896b3551b0a9c6affc33c7ea708 (glob)
  * syncing 21fa4d2997f1f2e050b3911639e9643efcb90896b3551b0a9c6affc33c7ea708 (glob)
  * changeset 21fa4d2997f1f2e050b3911639e9643efcb90896b3551b0a9c6affc33c7ea708 synced as f2f336ed26e996561dc4156ce6a5b647a30abe6b0584615c5739caaf8f6d153e * (glob)
  * successful sync bookmark update log #7 (glob)

-- check the state of bookmarks in the large repo
  $ mononoke_newadmin bookmarks --repo-name large list
  d52189c4c92bc5d3c99269e794afb72c31caf9ed4eba39abbb3c2739f010096d common_bookmark
  e3c1a924a6c56bf0dfbd0173601239a4776b6ed17075617270c27c80456e12fb main
  b149e7a688c2bac6a2f25dc4b846060f774967ae82eeac54cc1a7c148b215b27 small_repo_prefix/heads/common_bookmark
  f2f336ed26e996561dc4156ce6a5b647a30abe6b0584615c5739caaf8f6d153e small_repo_prefix/heads/other_bookmark
  ccca231a326f5060eebc66ed1f1ad6aaa1490f1d7faa40cc469e59bf5a4e1ee9 small_repo_prefix/tags/release_b

-- check the graph after all the syncing
  $ mononoke_newadmin changelog -R large graph -i d52189c4c92bc5d3c99269e794afb72c31caf9ed4eba39abbb3c2739f010096d,e3c1a924a6c56bf0dfbd0173601239a4776b6ed17075617270c27c80456e12fb,b149e7a688c2bac6a2f25dc4b846060f774967ae82eeac54cc1a7c148b215b27,ccca231a326f5060eebc66ed1f1ad6aaa1490f1d7faa40cc469e59bf5a4e1ee9,f2f336ed26e996561dc4156ce6a5b647a30abe6b0584615c5739caaf8f6d153e,540414777d82df622b2762dc79dfba2d92f994481105e34d47274c4742dbedb0 -M
  o  message: SC
  │
  o  message: SB
  │
  │ o  message: SE
  │ │
  o │  message: LC
  │ │
  │ │ o  message: LD
  │ │ │
  o │ │  message: LB
  ├───╯
  │ o  message: SD
  ├─╯
  │ o  message: SB
  ├─╯
  o  message: LA

-- check the mutable counters
$ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters where name = 'xreposync_from_1'";
