# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ DISALLOW_NON_PUSHREBASE=1 \
  > GLOBALREVS_PUBLISHING_BOOKMARK=master_bookmark \
  > EMIT_OBSMARKERS=1 \
  > BLOB_TYPE="blob_files" \
  > HGSQL_NAME=foorepo \
  > quiet default_setup
  $ hg up -q master_bookmark

Push commit, check a globalrev was assigned
  $ touch file1
  $ hg ci -Aqm commit1

  $ touch file2
  $ hg ci -Aqm commit2

  $ hgmn push -q -r . --to master_bookmark

  $ hg log -r '::.'  -T '{node}\n{extras % "{extra}\n"}\n'
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  branch=default
  
  112478962961147124edd43549aedd1a335e44bf
  branch=default
  
  26805aba1e600a82e93661149f2313866a221a7b
  branch=default
  
  2fa5be0dd895db0f33eaad12bc9fb3e17c169012
  branch=default
  global_rev=1000147970
  
  7a3a1e2e51f575e8390b4e0867ac8584dba59df8
  branch=default
  global_rev=1000147971
  

Initialize globalrevs DB

Sync a pushrebase bookmark move. This will fail because Globalrevs aren't initialized
  $ GLOBALREVS_DB="$TESTTMP/globalrevs"
  $ cd "$TESTTMP"
  $ mononoke_hg_sync repo-hg 1 --generate-bundles --hgsql-globalrevs-use-sqlite --hgsql-globalrevs-db-addr "$GLOBALREVS_DB" 2>&1 | grep "Attempted to move Globalrev"
      Attempted to move Globalrev for repository HgsqlGlobalrevsName("foorepo") backwards to 1000147972 (from None)

Update the repo. Sync again
  $ sqlite3 "$GLOBALREVS_DB" "INSERT INTO revision_references(repo, namespace, name, value) VALUES (CAST('foorepo' AS BLOB), 'counter', 'commit', 1);"
  $ quiet mononoke_hg_sync repo-hg 1 --generate-bundles --hgsql-globalrevs-use-sqlite --hgsql-globalrevs-db-addr "$GLOBALREVS_DB"

Check the DB
  $ sqlite3 "$GLOBALREVS_DB" "SELECT * FROM revision_references;"
  foorepo|counter|commit|1000147972

Set a non-globalrevs-publishing bookmark. Sync it.
  $ quiet mononoke_admin bookmarks set other_bookmark 2fa5be0dd895db0f33eaad12bc9fb3e17c169012
  $ quiet mononoke_hg_sync repo-hg 2 --generate-bundles --hgsql-globalrevs-use-sqlite --hgsql-globalrevs-db-addr "$GLOBALREVS_DB"

Check the DB again
  $ sqlite3 "$GLOBALREVS_DB" "SELECT * FROM revision_references;"
  foorepo|counter|commit|1000147972

Check that running without generate bundles fails
  $ mononoke_hg_sync repo-hg 1 --hgsql-globalrevs-use-sqlite --hgsql-globalrevs-db-addr "$GLOBALREVS_DB" 2>&1 | grep "Execution error"
  * Execution error: Syncing globalrevs (hgsql-globalrevs-db-addr) requires generating bundles (generate-bundles) (glob)
