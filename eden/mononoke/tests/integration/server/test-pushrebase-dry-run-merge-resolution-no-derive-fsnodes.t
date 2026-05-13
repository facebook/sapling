# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify that dry-run merge resolution logs skipped_fsnodes_not_derived
# when derive_fsnodes=false and fsnodes have NOT been derived.
#
# Setup: WBC derivation disabled, fsnodes NOT derived. The dry-run
# pre-check finds no fsnodes and skips, logging the reason to Scuba.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig push.edenapi=true

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:pushrebase_dry_run_merge_resolution": true,
  >     "scm/mononoke:pushrebase_enable_merge_resolution": false,
  >     "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes": false
  >   },
  >   "ints": {
  >     "scm/mononoke:pushrebase_max_merge_conflicts": 10,
  >     "scm/mononoke:pushrebase_max_merge_file_size": 10485760
  >   }
  > }
  > EOF

  $ SCUBA="$TESTTMP/scuba.json"
  $ BLOB_TYPE="blob_files" default_setup_drawdag --scuba-log-file "$SCUBA"
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Restart mononoke with WBC derivation disabled (no automatic fsnode derivation)
  $ killandwait $MONONOKE_PID
  $ start_and_wait_for_mononoke_server --enable-wbc-with no-derivation --scuba-log-file "$SCUBA"

Create a base file with multiple lines
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

NOTE: We do NOT derive fsnodes — that's the point of this test.

Server-side commit: modify the FIRST line
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

Clear scuba before the test push
  $ truncate -s 0 "$SCUBA"

Client commit: modify the LAST line (non-overlapping)
  $ hg up -q .~1
  $ cat > shared.txt << 'EOF'
  > line1
  > line2
  > line3
  > line4
  > CLIENT_EDIT_LINE5
  > EOF
  $ hg ci -m "client: edit line 5"

Push should FAIL — fsnodes not derived, dry-run skipped
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

Verify dry-run logged skipped_fsnodes_not_derived
  $ jq -S 'select(.normal.log_tag == "Pushrebase dry-run merge resolution") | {outcome: .normal.merge_dry_run_outcome}' < "$SCUBA"
  {
    "outcome": "skipped_fsnodes_not_derived"
  }
