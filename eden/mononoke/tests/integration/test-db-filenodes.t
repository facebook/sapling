# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ BLOB_TYPE="blob_files" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2


Check that filenodes exist after blobimport
  $ mononoke_admin filenodes validate master_bookmark &> /dev/null

Pushrebase commit 1
  $ hg up -q "min(all())"
  $ mkdir dir
  $ echo 1 > dir/1 && hg addremove -q && hg ci -m 1
  $ hg push -r . --to master_bookmark -q

Check that filenodes exist
  $ mononoke_admin filenodes validate master_bookmark &> /dev/null

Now delete, make sure validation fails
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from filenodes where repo_id >= 0"
  $ mononoke_admin filenodes validate master_bookmark &> /dev/null
  [1]
