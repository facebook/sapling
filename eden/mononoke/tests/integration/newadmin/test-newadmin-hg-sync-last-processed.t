# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config blob_files

setup stub data
  $ ZERO=0000000000000000000000000000000000000000000000000000000000000000
  $ ONE=1111111111111111111111111111111111111111111111111111111111111111
  $ TWO=2222222222222222222222222222222222222222222222222222222222222222
  $ THREE=3333333333333333333333333333333333333333333333333333333333333333
  $ FOUR=4444444444444444444444444444444444444444444444444444444444444444
  $ FIVE=5555555555555555555555555555555555555555555555555555555555555555
  $ SIX=6666666666666666666666666666666666666666666666666666666666666666
  $ SEVEN=7777777777777777777777777777777777777777777777777777777777777777
  $ EIGHT=8888888888888888888888888888888888888888888888888888888888888888
  $ NINE=9999999999999999999999999999999999999999999999999999999999999999
  $ AS=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ create_books_sqlite3_db
  $ mononoke_testtool modify-bookmark -R repo create master_bookmark --to "$ZERO"
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$ZERO" --to "$ONE"
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$ONE" --to "$TWO" --reason blobimport
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$TWO" --to "$THREE" --reason blobimport
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$THREE" --to "$FOUR"
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$FOUR" --to "$FIVE"
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$FIVE" --to "$SIX"
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$SIX" --to "$SEVEN" --reason blobimport
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$SEVEN" --to "$EIGHT" --reason blobimport
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select id, repo_id, hex(from_changeset_id), reason from bookmarks_update_log;"
  1|0||testmove
  2|0|0000000000000000000000000000000000000000000000000000000000000000|testmove
  3|0|1111111111111111111111111111111111111111111111111111111111111111|blobimport
  4|0|2222222222222222222222222222222222222222222222222222222222222222|blobimport
  5|0|3333333333333333333333333333333333333333333333333333333333333333|testmove
  6|0|4444444444444444444444444444444444444444444444444444444444444444|testmove
  7|0|5555555555555555555555555555555555555555555555555555555555555555|testmove
  8|0|6666666666666666666666666666666666666666666666666666666666666666|blobimport
  9|0|7777777777777777777777777777777777777777777777777777777777777777|blobimport
Check that we have no counter
  $ mononoke_newadmin hg-sync -R repo last-processed
  No counter found for repo (0)

Skipping ahead from the start of a series of regular changes should fail
  $ mononoke_newadmin hg-sync -R repo last-processed --set 1
  No counter found for repo (0)
  Counter for repo (0) set to 1
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport --dry-run
  Counter for repo (0) has value 1
  Error: No valid counter position to skip ahead to
  [1]
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport
  Counter for repo (0) has value 1
  Error: No valid counter position to skip ahead to
  [1]
Skipping ahead from the middle of a series of regular changes should fail (1)
  $ mononoke_newadmin hg-sync -R repo last-processed --set 5
  Counter for repo (0) has value 1
  Counter for repo (0) set to 5
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport --dry-run
  Counter for repo (0) has value 5
  Error: No valid counter position to skip ahead to
  [1]
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport
  Counter for repo (0) has value 5
  Error: No valid counter position to skip ahead to
  [1]
Skipping ahead from the middle of a series of regular changes should fail (2)
  $ mononoke_newadmin hg-sync -R repo last-processed --set 6
  Counter for repo (0) has value 5
  Counter for repo (0) set to 6
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport --dry-run
  Counter for repo (0) has value 6
  Error: No valid counter position to skip ahead to
  [1]
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport
  Counter for repo (0) has value 6
  Error: No valid counter position to skip ahead to
  [1]
Skipping ahead from the edge of a series of regular changes should fail
  $ mononoke_newadmin hg-sync -R repo last-processed --set 4
  Counter for repo (0) has value 6
  Counter for repo (0) set to 4
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport --dry-run
  Counter for repo (0) has value 4
  Error: No valid counter position to skip ahead to
  [1]
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport
  Counter for repo (0) has value 4
  Error: No valid counter position to skip ahead to
  [1]
Skipping ahead from the edge of a series of blobimports should succeed
  $ mononoke_newadmin hg-sync -R repo last-processed --set 2
  Counter for repo (0) has value 4
  Counter for repo (0) set to 2
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport --dry-run
  Counter for repo (0) has value 2
  Counter for repo (0) would be updated to 4
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport
  Counter for repo (0) has value 2
  Counter for repo (0) was updated to 4
Skipping ahead from the middle of a series of blobimports should succeed
  $ mononoke_newadmin hg-sync -R repo last-processed --set 3
  Counter for repo (0) has value 4
  Counter for repo (0) set to 3
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport --dry-run
  Counter for repo (0) has value 3
  Counter for repo (0) would be updated to 4
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport
  Counter for repo (0) has value 3
  Counter for repo (0) was updated to 4
Skipping ahead with No valid candidate should fail
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$EIGHT" --to "$NINE" --reason blobimport
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$NINE" --to "$AS" --reason blobimport
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select id, repo_id, hex(from_changeset_id), reason from bookmarks_update_log;"
  1|0||testmove
  2|0|0000000000000000000000000000000000000000000000000000000000000000|testmove
  3|0|1111111111111111111111111111111111111111111111111111111111111111|blobimport
  4|0|2222222222222222222222222222222222222222222222222222222222222222|blobimport
  5|0|3333333333333333333333333333333333333333333333333333333333333333|testmove
  6|0|4444444444444444444444444444444444444444444444444444444444444444|testmove
  7|0|5555555555555555555555555555555555555555555555555555555555555555|testmove
  8|0|6666666666666666666666666666666666666666666666666666666666666666|blobimport
  9|0|7777777777777777777777777777777777777777777777777777777777777777|blobimport
  10|0|8888888888888888888888888888888888888888888888888888888888888888|blobimport
  11|0|9999999999999999999999999999999999999999999999999999999999999999|blobimport
  $ mononoke_newadmin hg-sync -R repo last-processed --set 8
  Counter for repo (0) has value 4
  Counter for repo (0) set to 8
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport --dry-run
  Counter for repo (0) has value 8
  Error: No valid counter position to skip ahead to
  [1]
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport
  Counter for repo (0) has value 8
  Error: No valid counter position to skip ahead to
  [1]
It ignores unrelated repos when locating the first non-blobimport
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE bookmarks_update_log SET repo_id = 1 WHERE id > 4;"
  $ mononoke_newadmin hg-sync -R repo last-processed --set 2
  Counter for repo (0) has value 8
  Counter for repo (0) set to 2
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport --dry-run
  Counter for repo (0) has value 2
  Error: No valid counter position to skip ahead to
  [1]
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport
  Counter for repo (0) has value 2
  Error: No valid counter position to skip ahead to
  [1]
It ignores unrelated repos when locating the last blobimport
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE bookmarks_update_log SET repo_id = 0;"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE bookmarks_update_log SET repo_id = 1 WHERE id = 4;"
  $ mononoke_newadmin hg-sync -R repo last-processed --set 2
  Counter for repo (0) has value 2
  Counter for repo (0) set to 2
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport --dry-run
  Counter for repo (0) has value 2
  Counter for repo (0) would be updated to 3
  $ mononoke_newadmin hg-sync -R repo last-processed --skip-blobimport
  Counter for repo (0) has value 2
  Counter for repo (0) was updated to 3
