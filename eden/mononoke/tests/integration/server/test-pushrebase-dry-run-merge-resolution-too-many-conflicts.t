# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify that dry-run merge resolution logs too_many_conflicts when the
# number of conflicting files exceeds the max_merge_conflicts limit.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig push.edenapi=true

Enable dry-run with max 1 conflict file (so 2 conflicting files triggers skip)
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:pushrebase_dry_run_merge_resolution": true,
  >     "scm/mononoke:pushrebase_enable_merge_resolution": false,
  >     "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes": true
  >   },
  >   "ints": {
  >     "scm/mononoke:pushrebase_max_merge_conflicts": 1,
  >     "scm/mononoke:pushrebase_max_merge_file_size": 10485760
  >   }
  > }
  > EOF

  $ SCUBA="$TESTTMP/scuba.json"
  $ BLOB_TYPE="blob_files" default_setup_drawdag --scuba-log-file "$SCUBA"
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Clear startup scuba logs
  $ truncate -s 0 "$SCUBA"

Create two base files
  $ hg up -q "min(all())"
  $ echo "file1-base" > file1.txt
  $ echo "file2-base" > file2.txt
  $ hg add file1.txt file2.txt
  $ hg ci -m "add file1.txt and file2.txt"
  $ hg push -r . --to master_bookmark -q

Server-side commit: modify both files
  $ hg up -q master_bookmark
  $ echo "file1-server" > file1.txt
  $ echo "file2-server" > file2.txt
  $ hg ci -m "server: edit both files"
  $ hg push -r . --to master_bookmark -q

Clear scuba before test push
  $ truncate -s 0 "$SCUBA"

Client commit: also modify both files (2 conflicting files > limit of 1)
  $ hg up -q .~1
  $ echo "file1-client" > file1.txt
  $ echo "file2-client" > file2.txt
  $ hg ci -m "client: edit both files"

Push should FAIL
  $ hg push -r . --to master_bookmark
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue * for upload (glob)
  edenapi: uploaded * (glob)
  edenapi: queue * for upload (glob)
  edenapi: uploaded * (glob)
  edenapi: uploaded 1 changeset
  pushrebasing stack (*, *] (1 commit) to remote bookmark master_bookmark (glob)
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath("file1.txt"), right: MPath("file1.txt") }, PushrebaseConflict { left: MPath("file2.txt"), right: MPath("file2.txt") }]
  [255]

Verify dry-run logged too_many_conflicts
  $ jq -S 'select(.normal.log_tag == "Pushrebase dry-run merge resolution") | {outcome: .normal.merge_dry_run_outcome, total_conflicts: .int.merge_dry_run_total_conflicts, max_conflicts: .int.merge_dry_run_max_conflicts}' < "$SCUBA"
  {
    "max_conflicts": 1,
    "outcome": "too_many_conflicts",
    "total_conflicts": 2
  }
