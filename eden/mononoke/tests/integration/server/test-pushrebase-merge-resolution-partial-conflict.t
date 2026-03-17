# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify all-or-nothing semantics: if ANY file has a true conflict, the
# entire push is rejected even if other conflicting files are resolvable.
#
# mix_ok.txt has non-overlapping edits (would merge cleanly in isolation),
# but mix_bad.txt has overlapping edits. Because merge resolution cannot
# resolve all conflicts, pushrebase falls back to a conflict error.

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

  $ BLOB_TYPE="blob_files" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Create two base files
  $ hg up -q "min(all())"
  $ cat > mix_ok.txt << 'EOF'
  > ok_line1
  > ok_line2
  > ok_line3
  > EOF
  $ cat > mix_bad.txt << 'EOF'
  > bad_line1
  > bad_line2
  > bad_line3
  > EOF
  $ hg add mix_ok.txt mix_bad.txt
  $ hg ci -m "add mix files"
  $ hg push -r . --to master_bookmark -q

Server: edit line 1 of ok file (non-overlapping), line 2 of bad file
  $ hg up -q master_bookmark
  $ cat > mix_ok.txt << 'EOF'
  > SERVER_ok_line1
  > ok_line2
  > ok_line3
  > EOF
  $ cat > mix_bad.txt << 'EOF'
  > bad_line1
  > SERVER_bad_line2
  > bad_line3
  > EOF
  $ hg ci -m "server: edit mix files"
  $ hg push -r . --to master_bookmark -q

Client: edit line 3 of ok file (non-overlapping), line 2 of bad file (overlapping!)
  $ hg up -q .~1
  $ cat > mix_ok.txt << 'EOF'
  > ok_line1
  > ok_line2
  > CLIENT_ok_line3
  > EOF
  $ cat > mix_bad.txt << 'EOF'
  > bad_line1
  > CLIENT_bad_line2
  > bad_line3
  > EOF
  $ hg ci -m "client: edit mix files"

Pushrebase should FAIL — mix_bad.txt has a true conflict, so all files are rejected
  $ hg push -r . --to master_bookmark
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue * for upload (glob)
  edenapi: uploaded * (glob)
  edenapi: queue * for upload (glob)
  edenapi: uploaded * (glob)
  edenapi: uploaded 1 changeset
  pushrebasing stack (*, *] (1 commit) to remote bookmark master_bookmark (glob)
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath("mix_bad.txt"), right: MPath("mix_bad.txt") }, PushrebaseConflict { left: MPath("mix_ok.txt"), right: MPath("mix_ok.txt") }]
  [255]
