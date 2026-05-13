# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify that dry-run merge resolution logs file_too_large when a
# conflicting file exceeds the max file size limit.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig push.edenapi=true

Enable dry-run with a very small file size limit (10 bytes)
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:pushrebase_dry_run_merge_resolution": true,
  >     "scm/mononoke:pushrebase_enable_merge_resolution": false,
  >     "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes": true
  >   },
  >   "ints": {
  >     "scm/mononoke:pushrebase_max_merge_conflicts": 10,
  >     "scm/mononoke:pushrebase_max_merge_file_size": 10
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

Create a base file larger than 10 bytes
  $ hg up -q "min(all())"
  $ cat > shared.txt << 'EOF'
  > line1
  > line2
  > line3
  > line4
  > line5
  > EOF
  $ hg add shared.txt
  $ hg ci -m "add shared.txt"
  $ hg push -r . --to master_bookmark -q

Server-side commit: modify the first line
  $ hg up -q master_bookmark
  $ cat > shared.txt << 'EOF'
  > SERVER_EDIT_LINE1
  > line2
  > line3
  > line4
  > line5
  > EOF
  $ hg ci -m "server: edit line 1"
  $ hg push -r . --to master_bookmark -q

Clear scuba before test push
  $ truncate -s 0 "$SCUBA"

Client commit: modify the last line (non-overlapping, but file > 10 bytes)
  $ hg up -q .~1
  $ cat > shared.txt << 'EOF'
  > line1
  > line2
  > line3
  > line4
  > CLIENT_EDIT_LINE5
  > EOF
  $ hg ci -m "client: edit line 5"

Push should FAIL — file too large for dry-run merge
  $ hg push -r . --to master_bookmark
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue * for upload (glob)
  edenapi: uploaded * (glob)
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (*, *] (1 commit) to remote bookmark master_bookmark (glob)
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath("shared.txt"), right: MPath("shared.txt") }]
  [255]

Verify dry-run logged file_too_large with the file path
  $ jq -S 'select(.normal.log_tag == "Pushrebase dry-run merge resolution") | {outcome: .normal.merge_dry_run_outcome, file: .normal.merge_dry_run_file}' < "$SCUBA"
  {
    "file": "shared.txt",
    "outcome": "file_too_large"
  }
