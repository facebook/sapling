# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ DISALLOW_NON_PUSHREBASE=1 ASSIGN_GLOBALREVS=1 EMIT_OBSMARKERS=1 BLOB_TYPE="blob_files" quiet default_setup
  $ hg up -q master_bookmark

Push commit, check a globalrev was assigned
  $ touch file1
  $ hg ci -Aqm commit1
  $ hgmn push -q -r . --to master_bookmark
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  global_rev=1000147970

Initialize globalrevs DB

Sync a pushrebase bookmark move. This will fail because Globalrevs aren't initialized
  $ GLOBALREVS_DB="$TESTTMP/globalrevs"
  $ cd "$TESTTMP"
  $ mononoke_hg_sync repo-hg 1 --generate-bundles --hgsql-globalrevs-use-sqlite --hgsql-globalrevs-db-addr "$GLOBALREVS_DB" 2>&1 | grep "Attempted to move Globalrev"
      Attempted to move Globalrev for repository repo backwards to 1000147970

Update the repo. Sync again
  $ sqlite3 "$GLOBALREVS_DB" "INSERT INTO revision_references(repo, namespace, name, value) VALUES (CAST('repo' AS BLOB), 'counter', 'commit', 1);"
  $ mononoke_hg_sync repo-hg 1 --generate-bundles --hgsql-globalrevs-use-sqlite --hgsql-globalrevs-db-addr "$GLOBALREVS_DB" 2>&1 | grep "successful sync"
  * successful sync of entries [2] (glob)

Check the DB
  $ sqlite3 "$GLOBALREVS_DB" "SELECT * FROM revision_references;"
  repo|counter|commit|1000147970

Check that running wihtout generate bundles fails

  $ mononoke_hg_sync repo-hg 1 --hgsql-globalrevs-use-sqlite --hgsql-globalrevs-db-addr "$GLOBALREVS_DB" 2>&1 | grep "Execution error"
  * Execution error: Syncing globalrevs (hgsql-globalrevs-db-addr) requires generating bundles (generate-bundles) (glob)
