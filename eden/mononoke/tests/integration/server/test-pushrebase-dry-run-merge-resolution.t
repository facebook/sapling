# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify that dry-run merge resolution logs outcomes to Scuba correctly.
#
# Tests:
# 1. Non-overlapping edits → outcome=all_clean, resolved=1, conflicts=0
# 2. Overlapping edits → outcome=some_conflicts, conflicts=1
# 3. No conflict → dry-run not invoked (no log entry)

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig push.edenapi=true

Enable dry-run only (live merge resolution stays OFF)
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:pushrebase_dry_run_merge_resolution": true,
  >     "scm/mononoke:pushrebase_enable_merge_resolution": false,
  >     "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes": true
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

Clear startup scuba logs
  $ truncate -s 0 "$SCUBA"

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

Clear scuba before the test pushes
  $ truncate -s 0 "$SCUBA"

-- Test 1: Non-overlapping edits → dry-run logs all_clean but push still fails --
  $ hg up -q .~1
  $ cat > shared.txt << 'EOF'
  > line1
  > line2
  > line3
  > line4
  > CLIENT_EDIT_LINE5
  > EOF
  $ hg ci -m "client: edit line 5 (non-overlapping)"

Push should FAIL (live merge is off), but dry-run should log all_clean
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

Verify dry-run logged all_clean with resolved=1
  $ jq -S 'select(.normal.log_tag == "Pushrebase dry-run merge resolution") | {outcome: .normal.merge_dry_run_outcome, resolved: .int.merge_dry_run_resolved, conflicts: .int.merge_dry_run_conflicts, skipped: .int.merge_dry_run_skipped, errors: .int.merge_dry_run_errors}' < "$SCUBA"
  {
    "conflicts": 0,
    "errors": 0,
    "outcome": "all_clean",
    "resolved": 1,
    "skipped": 0
  }

Clear scuba for next test
  $ truncate -s 0 "$SCUBA"

-- Test 2: Overlapping edits → dry-run logs some_conflicts --
  $ hg up -q .~1
  $ cat > shared.txt << 'EOF'
  > CLIENT_ALSO_EDITS_LINE1
  > line2
  > line3
  > line4
  > line5
  > EOF
  $ hg ci -m "client: edit line 1 (overlapping with server)"

Push should FAIL — both sides edited line 1
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

Verify dry-run logged some_conflicts with conflicts=1
  $ jq -S 'select(.normal.log_tag == "Pushrebase dry-run merge resolution") | {outcome: .normal.merge_dry_run_outcome, resolved: .int.merge_dry_run_resolved, conflicts: .int.merge_dry_run_conflicts, skipped: .int.merge_dry_run_skipped, errors: .int.merge_dry_run_errors}' < "$SCUBA"
  {
    "conflicts": 1,
    "errors": 0,
    "outcome": "some_conflicts",
    "resolved": 0,
    "skipped": 0
  }

Clear scuba for next test
  $ truncate -s 0 "$SCUBA"

-- Test 3: No conflict — dry-run not invoked, no log entry --
  $ hg up -q master_bookmark
  $ cat > noconflict.txt << 'EOF'
  > this file doesn't conflict
  > EOF
  $ hg add noconflict.txt
  $ hg ci -m "add a non-conflicting file"
  $ hg push -r . --to master_bookmark
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue * for upload (glob)
  edenapi: uploaded * (glob)
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (*, *] (1 commit) to remote bookmark master_bookmark (glob)
  * files updated, 0 files merged, 0 files removed, 0 files unresolved (glob)
  updated remote bookmark master_bookmark to * (glob)

No dry-run log entry should exist (no conflict = no dry-run)
  $ jq -S 'select(.normal.log_tag == "Pushrebase dry-run merge resolution")' < "$SCUBA"
