# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify that the per-push merge-resolution summary lands on the
# pushrebase Scuba sample as the `mr_outcome` /
# `mr_conflict_files_count` / `mr_resolved_files_count` columns.
#
# Two scenarios driven through the same Scuba sink:
#   1. Clean pushes with no conflicts -> mr_outcome=not_needed
#   2. A conflicting push that MR resolves -> mr_outcome=succeeded
#      with mr_resolved_files_count == 1.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig push.edenapi=true

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:pushrebase_enable_merge_resolution": true,
  >     "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes": true
  >   },
  >   "ints": {
  >     "scm/mononoke:pushrebase_max_merge_conflicts": 10,
  >     "scm/mononoke:pushrebase_max_merge_file_size": 10485760
  >   }
  > }
  > EOF

  $ BLOB_TYPE="blob_files" default_setup_drawdag --scuba-log-file "$TESTTMP/scuba.json"
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

-- Setup: create a base file with multiple lines so the 3-way merge
-- has room for non-overlapping edits.
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

-- Server edits the FIRST line.
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

-- Client (from pre-server base) edits the LAST line. Pushrebase needs
-- merge resolution to land this against the server-side edit.
  $ hg up -q .~1
  $ cat > shared.txt << 'EOF'
  > line1
  > line2
  > line3
  > line4
  > CLIENT_EDIT_LINE5
  > EOF
  $ hg ci -m "client: edit line 5"
  $ hg push -r . --to master_bookmark -q

-- Verify the per-push pushrebase Scuba sample carries the new
-- `mr_outcome` column. We expect at least one `not_needed` (the
-- non-conflicting pushes) and exactly one `succeeded` (the client
-- edit that needed MR).
  $ jq -r 'select(.normal.log_tag == "pushrebase_complete") | .normal.mr_outcome' "$TESTTMP/scuba.json" | sort | uniq -c | awk '{print $2": "$1}'
  not_needed: 2
  succeeded: 1

-- For the succeeded sample, mr_resolved_files_count must equal the
-- number of files MR resolved. Only one file (shared.txt) was conflicting.
  $ jq -r 'select(.normal.log_tag == "pushrebase_complete" and .normal.mr_outcome == "succeeded") | .int.mr_resolved_files_count' "$TESTTMP/scuba.json"
  1

-- Sanity: not_needed samples report zero counts.
  $ jq -r 'select(.normal.log_tag == "pushrebase_complete" and .normal.mr_outcome == "not_needed") | .int.mr_resolved_files_count' "$TESTTMP/scuba.json" | sort -u
  0

-- The pushes above carried no MERGE_RESOLUTION_OVERRIDE pushvar, so they
-- defer to the JK and are labeled `bypass` (out-of-experiment / not an
-- assigned QE arm) on the `mr_qe_arm` column.
  $ jq -r 'select(.normal.log_tag == "pushrebase_complete") | .normal.mr_qe_arm' "$TESTTMP/scuba.json" | sort -u
  bypass

-- Exercise the QE arm labels end-to-end via the pushvar. A clean
-- (non-conflicting) push with MERGE_RESOLUTION_OVERRIDE=true is the
-- treatment arm -> mr_qe_arm=test; =false is the control arm ->
-- mr_qe_arm=control. Both land because there is no conflict.
  $ hg up -q master_bookmark
  $ echo test_arm_line >> shared.txt
  $ hg ci -m "client: append (test arm)"
  $ hg push -r . --to master_bookmark -q --pushvar MERGE_RESOLUTION_OVERRIDE=true

  $ hg up -q master_bookmark
  $ echo control_arm_line >> shared.txt
  $ hg ci -m "client: append (control arm)"
  $ hg push -r . --to master_bookmark -q --pushvar MERGE_RESOLUTION_OVERRIDE=false

-- Cumulative arm distribution: the 3 override-free pushes are `bypass`,
-- plus one `test` and one `control` from the pushvar-driven pushes.
  $ jq -r 'select(.normal.log_tag == "pushrebase_complete") | .normal.mr_qe_arm' "$TESTTMP/scuba.json" | sort | uniq -c | awk '{print $2": "$1}'
  bypass: 3
  control: 1
  test: 1
