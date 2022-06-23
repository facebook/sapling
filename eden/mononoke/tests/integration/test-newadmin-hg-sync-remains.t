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

setup configuration
  $ setup_common_config blob_files
  $ create_books_sqlite3_db
  $ mononoke_testtool modify-bookmark -R repo create master_bookmark --to "$ZERO"
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --from "$ZERO" --to "$ONE"
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --reason blobimport --from "$ONE" --to "$TWO"
  $ mononoke_testtool modify-bookmark -R repo update master_bookmark --reason blobimport --from "$TWO" --to "$THREE"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select id, repo_id, hex(from_changeset_id), reason from bookmarks_update_log;"
  1|0||testmove
  2|0|0000000000000000000000000000000000000000000000000000000000000000|testmove
  3|0|1111111111111111111111111111111111111111111111111111111111111111|blobimport
  4|0|2222222222222222222222222222222222222222222222222222222222222222|blobimport

it should count remaining entries
  $ mononoke_newadmin hg-sync -R repo last-processed --set 0
  No counter found for repo (0)
  Counter for repo (0) set to 0
  $ mononoke_newadmin hg-sync -R repo remains
  Remaining bundles to replay in repo (0): 4
  $ mononoke_newadmin hg-sync -R repo last-processed --set 1
  Counter for repo (0) has value 0
  Counter for repo (0) set to 1
  $ mononoke_newadmin hg-sync -R repo remains
  Remaining bundles to replay in repo (0): 3
  $ mononoke_newadmin hg-sync -R repo last-processed --set 10
  Counter for repo (0) has value 1
  Counter for repo (0) set to 10
  $ mononoke_newadmin hg-sync -R repo remains
  Remaining bundles to replay in repo (0): 0

it should count remaining entries excluding blobimport
  $ mononoke_newadmin hg-sync -R repo last-processed --set 0
  Counter for repo (0) has value 10
  Counter for repo (0) set to 0
  $ mononoke_newadmin hg-sync -R repo remains --without-blobimport
  Remaining non-blobimport bundles to replay in repo (0): 2
  $ mononoke_newadmin hg-sync -R repo last-processed --set 1
  Counter for repo (0) has value 0
  Counter for repo (0) set to 1
  $ mononoke_newadmin hg-sync -R repo remains --without-blobimport
  Remaining non-blobimport bundles to replay in repo (0): 1
  $ mononoke_newadmin hg-sync -R repo last-processed --set 10
  Counter for repo (0) has value 1
  Counter for repo (0) set to 10
  $ mononoke_newadmin hg-sync -R repo remains --without-blobimport
  Remaining non-blobimport bundles to replay in repo (0): 0

it should support --quiet
  $ mononoke_newadmin hg-sync -R repo last-processed --set 0
  Counter for repo (0) has value 10
  Counter for repo (0) set to 0
  $ mononoke_newadmin hg-sync -R repo remains --quiet
  4
  $ mononoke_newadmin hg-sync -R repo remains --quiet --without-blobimport
  2
