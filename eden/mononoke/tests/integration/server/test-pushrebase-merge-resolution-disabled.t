# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify that merge resolution is gated by the JustKnob.
#
# With pushrebase_enable_merge_resolution=false, even non-overlapping edits
# to the same file produce a path-level conflict and the push is rejected.
# This confirms the feature is safely off by default and only activates
# when explicitly enabled.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig push.edenapi=true

Explicitly disable merge resolution (this is also the default)
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:pushrebase_enable_merge_resolution": false,
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
  $ cat > disabled_test.txt << 'EOF'
  > dt_line1
  > dt_line2
  > dt_line3
  > EOF
  $ hg add disabled_test.txt
  $ hg ci -m "add disabled_test.txt"
  $ hg push -r . --to master_bookmark -q

Server edits line 1
  $ hg up -q master_bookmark
  $ cat > disabled_test.txt << 'EOF'
  > SERVER_dt_line1
  > dt_line2
  > dt_line3
  > EOF
  $ hg ci -m "server: edit disabled_test"
  $ hg push -r . --to master_bookmark -q

Client edits line 3 (non-overlapping — would be resolvable if enabled)
  $ hg up -q .~1
  $ cat > disabled_test.txt << 'EOF'
  > dt_line1
  > dt_line2
  > CLIENT_dt_line3
  > EOF
  $ hg ci -m "client: edit disabled_test"

Pushrebase should FAIL — merge resolution is disabled, so any path
conflict is rejected regardless of whether the edits overlap
  $ hg push -r . --to master_bookmark
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue * for upload (glob)
  edenapi: uploaded * (glob)
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (*, *] (1 commit) to remote bookmark master_bookmark (glob)
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath("disabled_test.txt"), right: MPath("disabled_test.txt") }]
  [255]
