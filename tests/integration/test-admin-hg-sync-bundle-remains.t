  $ . "${TEST_FIXTURES}/library.sh"
  $ ZERO=0000000000000000000000000000000000000000000000000000000000000000
  $ ONE=1111111111111111111111111111111111111111111111111111111111111111
  $ TWO=2222222222222222222222222222222222222222222222222222222222222222
  $ THREE=3333333333333333333333333333333333333333333333333333333333333333

setup configuration
  $ ENABLE_PRESERVE_BUNDLE2=1 setup_common_config blob:files
  $ create_books_sqlite3_db
  $ write_stub_log_entry create "$ZERO"
  $ write_stub_log_entry update "$ZERO" "$ONE"
  $ write_stub_log_entry --blobimport update "$ONE" "$TWO"
  $ write_stub_log_entry --blobimport update "$TWO" "$THREE"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select id, repo_id, hex(from_changeset_id), reason from bookmarks_update_log;"
  1|0||testmove
  2|0|0000000000000000000000000000000000000000000000000000000000000000|testmove
  3|0|1111111111111111111111111111111111111111111111111111111111111111|blobimport
  4|0|2222222222222222222222222222222222222222222222222222222222222222|blobimport

it should count remaining entries
  $ mononoke_admin hg-sync-bundle last-processed --set 0
  * Counter for RepositoryId(0) set to 0 (glob)
  $ mononoke_admin hg-sync-bundle remains
  * Remaining bundles to replay in RepositoryId(0): 4 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --set 1
  * Counter for RepositoryId(0) set to 1 (glob)
  $ mononoke_admin hg-sync-bundle remains
  * Remaining bundles to replay in RepositoryId(0): 3 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --set 10
  * Counter for RepositoryId(0) set to 10 (glob)
  $ mononoke_admin hg-sync-bundle remains
  * Remaining bundles to replay in RepositoryId(0): 0 (glob)
it should count remaining entries excluding blobimport
  $ mononoke_admin hg-sync-bundle last-processed --set 0
  * Counter for RepositoryId(0) set to 0 (glob)
  $ mononoke_admin hg-sync-bundle remains --without-blobimport
  * Remaining non-blobimport bundles to replay in RepositoryId(0): 2 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --set 1
  * Counter for RepositoryId(0) set to 1 (glob)
  $ mononoke_admin hg-sync-bundle remains --without-blobimport
  * Remaining non-blobimport bundles to replay in RepositoryId(0): 1 (glob)
  $ mononoke_admin hg-sync-bundle last-processed --set 10
  * Counter for RepositoryId(0) set to 10 (glob)
  $ mononoke_admin hg-sync-bundle remains --without-blobimport
  * Remaining non-blobimport bundles to replay in RepositoryId(0): 0 (glob)
it should support --quiet
  $ mononoke_admin hg-sync-bundle last-processed --set 0
  * Counter for RepositoryId(0) set to 0 (glob)
  $ mononoke_admin hg-sync-bundle remains --quiet
  4
  $ mononoke_admin hg-sync-bundle remains --quiet --without-blobimport
  2
