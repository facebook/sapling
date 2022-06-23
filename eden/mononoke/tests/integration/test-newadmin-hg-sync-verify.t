# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ ZERO=0000000000000000000000000000000000000000000000000000000000000000
  $ ONE=1111111111111111111111111111111111111111111111111111111111111111
  $ TWO=2222222222222222222222222222222222222222222222222222222222222222
  $ THREE=3333333333333333333333333333333333333333333333333333333333333333
  $ FOUR=4444444444444444444444444444444444444444444444444444444444444444

setup configuration
  $ setup_common_config blob_files
  $ create_books_sqlite3_db
  $ mononoke_testtool modify-bookmark -R repo create master_bookmark --to "$ZERO"
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$ZERO" --to "$ONE"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select id, repo_id, hex(from_changeset_id), reason from bookmarks_update_log;"
  1|0||testmove
  2|0|0000000000000000000000000000000000000000000000000000000000000000|testmove

it defaults to zero
  $ mononoke_newadmin hg-sync -R repo verify
  All remaining bundles in repo (0) are non-blobimports (found 2)

it is satisfied with only non-blobimport entries
  $ mononoke_newadmin hg-sync -R repo last-processed --set 1
  No counter found for repo (0)
  Counter for repo (0) set to 1
  $ mononoke_newadmin hg-sync -R repo verify
  All remaining bundles in repo (0) are non-blobimports (found 1)

it is satisfied with only blobimport entries
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$ONE" --to "$TWO" --reason blobimport
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$TWO" --to "$THREE" --reason blobimport
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select id, repo_id, hex(from_changeset_id), reason from bookmarks_update_log;"
  1|0||testmove
  2|0|0000000000000000000000000000000000000000000000000000000000000000|testmove
  3|0|1111111111111111111111111111111111111111111111111111111111111111|blobimport
  4|0|2222222222222222222222222222222222222222222222222222222222222222|blobimport
  $ mononoke_newadmin hg-sync -R repo last-processed --set 2
  Counter for repo (0) has value 1
  Counter for repo (0) set to 2
  $ mononoke_newadmin hg-sync -R repo verify
  All remaining bundles in repo (0) are blobimports (found 2)

it reports a conflict
  $ mononoke_newadmin hg-sync -R repo last-processed --set 1
  Counter for repo (0) has value 2
  Counter for repo (0) set to 1
  $ mononoke_newadmin hg-sync -R repo verify
  Remaining bundles to replay in repo (0) are not consistent: found 2 blobimports and 1 non-blobimports

it reports correctly when there is nothing to be found
  $ mononoke_newadmin hg-sync -R repo last-processed --set 10
  Counter for repo (0) has value 1
  Counter for repo (0) set to 10
  $ mononoke_newadmin hg-sync -R repo verify
  No replay data found in repo (0)
