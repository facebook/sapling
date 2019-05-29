  $ . $TESTDIR/library.sh
  $ ZERO=0000000000000000000000000000000000000000000000000000000000000000
  $ ONE=1111111111111111111111111111111111111111111111111111111111111111
  $ TWO=2222222222222222222222222222222222222222222222222222222222222222
  $ THREE=3333333333333333333333333333333333333333333333333333333333333333
  $ FOUR=4444444444444444444444444444444444444444444444444444444444444444

setup configuration
  $ ENABLE_PRESERVE_BUNDLE2=1 setup_common_config blob:files
  $ create_books_sqlite3_db
  $ write_stub_log_entry create "$ZERO"
  $ write_stub_log_entry update "$ZERO" "$ONE"
  $ sqlite3 "$TESTTMP/repo/bookmarks" "select id, repo_id, hex(from_changeset_id), reason from bookmarks_update_log;"
  1|0||testmove
  2|0|0000000000000000000000000000000000000000000000000000000000000000|testmove

it defaults to zero
  $ mononoke_admin hg-sync-bundle verify
  * INFO All remaining bundles in RepositoryId(0) are non-blobimports (found 2) (glob)

it is satisfied with only non-blobimport entries
  $ mononoke_admin hg-sync-bundle last-processed --set 1
  * INFO Counter for RepositoryId(0) set to 1 (glob)
  $ mononoke_admin hg-sync-bundle verify
  * INFO All remaining bundles in RepositoryId(0) are non-blobimports (found 1) (glob)
it is satisfied with only blobimport entries
  $ write_stub_log_entry --blobimport update "$ONE" "$TWO"
  $ write_stub_log_entry --blobimport update "$TWO" "$THREE"
  $ sqlite3 "$TESTTMP/repo/bookmarks" "select id, repo_id, hex(from_changeset_id), reason from bookmarks_update_log;"
  1|0||testmove
  2|0|0000000000000000000000000000000000000000000000000000000000000000|testmove
  3|0|1111111111111111111111111111111111111111111111111111111111111111|blobimport
  4|0|2222222222222222222222222222222222222222222222222222222222222222|blobimport
  $ mononoke_admin hg-sync-bundle last-processed --set 2
  * INFO Counter for RepositoryId(0) set to 2 (glob)
  $ mononoke_admin hg-sync-bundle verify
  * INFO All remaining bundles in RepositoryId(0) are blobimports (found 2) (glob)
it reports a conflict
  $ mononoke_admin hg-sync-bundle last-processed --set 1
  * INFO Counter for RepositoryId(0) set to 1 (glob)
  $ mononoke_admin hg-sync-bundle verify
  * INFO Remaining bundles to replay in RepositoryId(0) are not consistent: found 2 blobimports and 1 non-blobimports (glob)
it nothing to be found
  $ mononoke_admin hg-sync-bundle last-processed --set 10
  * INFO Counter for RepositoryId(0) set to 10 (glob)
  $ mononoke_admin hg-sync-bundle verify
  * INFO No replay data found in RepositoryId(0) (glob)
