# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repository

  $ export SCUBA_LOGGING_PATH="$TESTTMP/scuba.json"
  $ export REPO_CLIENT_USE_WARM_BOOKMARKS_CACHE="true"
  $ export WARM_BOOKMARK_CACHE_CHECK_BLOBIMPORT="true"
  $ BLOB_TYPE="blob_files" quiet default_setup_pre_blobimport

Do a few tricks here:
Do blobimport without importing any bookmarks, and then manually move bookmark twice.
This is to have two entries in bookmark update log history.
Then set highest-imported-gen-num to 2, so that it looked as if only second commit is blobimported
  $ blobimport repo-hg/.hg repo --no-bookmark
  $ cd repo-hg
  $ hg log -r 1 -T '{node}\n'
  112478962961147124edd43549aedd1a335e44bf
  $ hg log -r 2 -T '{node}\n'
  26805aba1e600a82e93661149f2313866a221a7b
  $ cd "$TESTTMP"
  $ mononoke_admin bookmarks set master_bookmark 112478962961147124edd43549aedd1a335e44bf &> /dev/null
  $ mononoke_admin bookmarks set master_bookmark 26805aba1e600a82e93661149f2313866a221a7b &> /dev/null
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters";
  0|highest-imported-gen-num|3
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from mutable_counters where repo_id=0";
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "insert into mutable_counters (repo_id, name, value) values(0, 'highest-imported-gen-num', 2)";

  $ mononoke "$@"
  $ wait_for_mononoke "$TESTTMP/repo"
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  $ cd repo2 || exit 1
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

  $ cd "$TESTTMP/repo2"
  $ hg log -r "master_bookmark" -T '{desc}\n'
  C
Pull should return a bookmark that points to commit B
  $ hgmn pull -q
  devel-warn: applied empty changegroup at: * (glob)
  $ hg log -r "master_bookmark" -T '{desc}\n'
  B
Now update highest-imported-gen-num and pull again
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from mutable_counters where repo_id=0";
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "insert into mutable_counters (repo_id, name, value) values(0, 'highest-imported-gen-num', 3)";
  $ sleep 2
  $ hgmn pull -q
  devel-warn: applied empty changegroup at: * (glob)
  $ hg log -r "master_bookmark" -T '{desc}\n'
  C

  $ hgmn up -q 0
  $ echo a >> anotherfile
  $ hg add anotherfile
  $ hg ci -m 'new commit'
  $ hg log -r master_bookmark -T '{node}\n'
  26805aba1e600a82e93661149f2313866a221a7b
  $ hgmn push -r . --to master_bookmark
  pushing rev b1673e56df82 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  $ hg log -r master_bookmark -T '{node}\n'
  3dee7c6d777101a0f12a87a1394b35b4a249c700

  $ sleep 2
  $ grep "Fetching bookmarks from Warm bookmarks cache" "$SCUBA_LOGGING_PATH" | wc -l
  3

  $ hgmn pull -q
  devel-warn: applied empty changegroup at: * (glob)
  $ grep "Fetching bookmarks from Warm bookmarks cache" "$SCUBA_LOGGING_PATH" | wc -l
  4
