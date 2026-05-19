# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify that the `MERGE_RESOLUTION_OVERRIDE` pushvar can force merge
# resolution OFF even when the JustKnob is ON. This is the QE rollout
# control-arm path: Sandcastle sets the pushvar per-request to disable
# MR for the control population so we can causally measure its effect.
#
# Setup: JK pushrebase_enable_merge_resolution=true (so MR is ON by
# default). Without the pushvar, a non-overlapping edit would be
# 3-way-merged cleanly (this baseline is verified in
# test-pushrebase-merge-resolution-clean.t). With the pushvar set to
# "false", the override wins and the conflict is rejected as if MR
# were disabled.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig push.edenapi=true

JK on — without the pushvar, non-overlapping edits would merge cleanly
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

  $ BLOB_TYPE="blob_files" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Create a base file
  $ hg up -q "min(all())"
  $ cat > control_test.txt << 'EOF'
  > ct_line1
  > ct_line2
  > ct_line3
  > EOF
  $ hg add control_test.txt
  $ hg ci -m "add control_test.txt"
  $ hg push -r . --to master_bookmark -q

Server edits line 1
  $ hg up -q master_bookmark
  $ cat > control_test.txt << 'EOF'
  > SERVER_ct_line1
  > ct_line2
  > ct_line3
  > EOF
  $ hg ci -m "server: edit control_test"
  $ hg push -r . --to master_bookmark -q

Client edits line 3 (non-overlapping — would be 3-way merged if pushvar absent)
  $ hg up -q .~1
  $ cat > control_test.txt << 'EOF'
  > ct_line1
  > ct_line2
  > CLIENT_ct_line3
  > EOF
  $ hg ci -m "client: edit control_test"

Push WITH the override pushvar set to "false" — even though the JK is on,
the override forces merge resolution OFF and the path conflict is rejected
  $ hg push -r . --to master_bookmark --pushvar MERGE_RESOLUTION_OVERRIDE=false
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue * for upload (glob)
  edenapi: uploaded * (glob)
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (*, *] (1 commit) to remote bookmark master_bookmark (glob)
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath("control_test.txt"), right: MPath("control_test.txt") }]
  [255]
