  $ . $TESTDIR/library.sh

setup configuration
  $ ENABLE_PRESERVE_BUNDLE2=1 setup_common_config blob:files
  $ mkdir "$TESTTMP"/repo

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
  $ write_stub_log_entry create "$ZERO"
  $ write_stub_log_entry update "$ZERO" "$ONE"
  $ write_stub_log_entry --blobimport update "$ONE" "$TWO"
  $ write_stub_log_entry --blobimport update "$TWO" "$THREE"
  $ write_stub_log_entry update "$THREE" "$FOUR"
  $ write_stub_log_entry update "$FOUR" "$FIVE"
  $ write_stub_log_entry update "$FIVE" "$SIX"
  $ write_stub_log_entry --blobimport update "$SIX" "$SEVEN"
  $ write_stub_log_entry --blobimport update "$SEVEN" "$EIGHT"
  $ sqlite3 "$TESTTMP/repo/bookmarks" "select id, repo_id, hex(from_changeset_id), reason from bookmarks_update_log;"
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
  $ mononoke_admin hg-sync-bundle last-processed
  * INFO No counter found for RepositoryId(0) (glob)

Check that conflicting commands fail
  $ mononoke_admin hg-sync-bundle last-processed --set 0 --skip-blobimport
  * ERRO ErrorMessage { msg: "cannot pass both --set and --skip-blobimport" } (glob)
  [1]
  $ mononoke_admin hg-sync-bundle last-processed --set 0 --dry-run
  * ERRO ErrorMessage { msg: "--dry-run is meaningless without --skip-blobimport" } (glob)
  [1]
  $ mononoke_admin hg-sync-bundle last-processed --dry-run
  * ERRO ErrorMessage { msg: "--dry-run is meaningless without --skip-blobimport" } (glob)
  [1]

Skipping ahead from the start of a series of regular changes should fail
  $ mononoke_admin hg-sync-bundle last-processed --set 1
  * INFO Counter for RepositoryId(0) set to 1 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport --dry-run
  * INFO Counter for RepositoryId(0) has value 1 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport
  * INFO Counter for RepositoryId(0) has value 1 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
Skipping ahead from the middle of a series of regular changes should fail (1)
  $ mononoke_admin hg-sync-bundle last-processed --set 5
  * INFO Counter for RepositoryId(0) set to 5 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport --dry-run
  * INFO Counter for RepositoryId(0) has value 5 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport
  * INFO Counter for RepositoryId(0) has value 5 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
Skipping ahead from the middle of a series of regular changes should fail (2)
  $ mononoke_admin hg-sync-bundle last-processed --set 6
  * INFO Counter for RepositoryId(0) set to 6 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport --dry-run
  * INFO Counter for RepositoryId(0) has value 6 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport
  * INFO Counter for RepositoryId(0) has value 6 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
Skipping ahead from the edge of a series of regular changes should fail
  $ mononoke_admin hg-sync-bundle last-processed --set 4
  * INFO Counter for RepositoryId(0) set to 4 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport --dry-run
  * INFO Counter for RepositoryId(0) has value 4 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport
  * INFO Counter for RepositoryId(0) has value 4 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
Skipping ahead from the edge of a series of blobimports should succeed
  $ mononoke_admin hg-sync-bundle last-processed --set 2
  * INFO Counter for RepositoryId(0) set to 2 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport --dry-run
  * INFO Counter for RepositoryId(0) has value 2 (glob)
  * INFO Counter for RepositoryId(0) would be updated to 4 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport
  * INFO Counter for RepositoryId(0) has value 2 (glob)
  * INFO Counter for RepositoryId(0) was updated to 4 (glob)
Skipping ahead from the middle of a series of blobimports should succeed
  $ mononoke_admin hg-sync-bundle last-processed --set 3
  * INFO Counter for RepositoryId(0) set to 3 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport --dry-run
  * INFO Counter for RepositoryId(0) has value 3 (glob)
  * INFO Counter for RepositoryId(0) would be updated to 4 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport
  * INFO Counter for RepositoryId(0) has value 3 (glob)
  * INFO Counter for RepositoryId(0) was updated to 4 (glob)
Skipping ahead with no valid candidate should fail
  $ write_stub_log_entry --blobimport update "$EIGHT" "$NINE"
  $ write_stub_log_entry --blobimport update "$NINE" "$AS"
  $ sqlite3 "$TESTTMP/repo/bookmarks" "select id, repo_id, hex(from_changeset_id), reason from bookmarks_update_log;"
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
  $ mononoke_admin hg-sync-bundle last-processed --set 8
  * INFO Counter for RepositoryId(0) set to 8 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport --dry-run
  * INFO Counter for RepositoryId(0) has value 8 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport
  * INFO Counter for RepositoryId(0) has value 8 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
It ignores unrelated repos when locating the first non-blobimport
  $ sqlite3 "$TESTTMP/repo/bookmarks" "UPDATE bookmarks_update_log SET repo_id = 1 WHERE id > 4;"
  $ mononoke_admin hg-sync-bundle last-processed --set 2
  * INFO Counter for RepositoryId(0) set to 2 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport --dry-run
  * INFO Counter for RepositoryId(0) has value 2 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport
  * INFO Counter for RepositoryId(0) has value 2 (glob)
  * ERRO ErrorMessage { msg: "no valid counter position to skip ahead to" } (glob)
  [1]
It ignores unrelated repos when locating the last blobimport
  $ sqlite3 "$TESTTMP/repo/bookmarks" "UPDATE bookmarks_update_log SET repo_id = 0;"
  $ sqlite3 "$TESTTMP/repo/bookmarks" "UPDATE bookmarks_update_log SET repo_id = 1 WHERE id = 4;"
  $ mononoke_admin hg-sync-bundle last-processed --set 2
  * INFO Counter for RepositoryId(0) set to 2 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport --dry-run
  * INFO Counter for RepositoryId(0) has value 2 (glob)
  * INFO Counter for RepositoryId(0) would be updated to 3 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --skip-blobimport
  * INFO Counter for RepositoryId(0) has value 2 (glob)
  * INFO Counter for RepositoryId(0) was updated to 3 (glob)
