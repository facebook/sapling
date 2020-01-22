# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'

Check that filenodes exist after blobimport
  $ mononoke_admin filenodes validate master_bookmark &> /dev/null

Pushrebase commit 1
  $ hg up -q 0
  $ mkdir dir
  $ echo 1 > dir/1 && hg addremove -q && hg ci -m 1
  $ hgmn push -r . --to master_bookmark -q

Check that filenodes exist
  $ mononoke_admin filenodes validate master_bookmark &> /dev/null

Now delete, make sure validation fails
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from filenodes where repo_id >= 0"
  $ mononoke_admin filenodes validate master_bookmark &> /dev/null
  [1]

