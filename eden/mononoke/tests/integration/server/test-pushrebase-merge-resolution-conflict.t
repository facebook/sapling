# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify that pushrebase merge resolution rejects true content conflicts.
#
# When both sides modify the same line of a file, the 3-way merge detects
# overlapping edits and the push is rejected with a PushrebaseConflict error,
# even with merge resolution enabled.

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

Create a base file
  $ hg up -q "min(all())"
  $ cat > conflict.txt << 'EOF'
  > line1
  > line2
  > line3
  > EOF
  $ hg add conflict.txt
  $ hg ci -m "add conflict.txt"
  $ hg push -r . --to master_bookmark -q

Server edits line 2
  $ hg up -q master_bookmark
  $ cat > conflict.txt << 'EOF'
  > line1
  > SERVER_EDIT
  > line3
  > EOF
  $ hg ci -m "server: edit line 2"
  $ hg push -r . --to master_bookmark -q

Client also edits line 2 differently (from pre-server base)
  $ hg up -q .~1
  $ cat > conflict.txt << 'EOF'
  > line1
  > CLIENT_EDIT
  > line3
  > EOF
  $ hg ci -m "client: edit line 2"

Pushrebase should FAIL — overlapping edits are a true conflict
  $ hg push -r . --to master_bookmark
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue * for upload (glob)
  edenapi: uploaded * (glob)
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (*, *] (1 commit) to remote bookmark master_bookmark (glob)
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath("conflict.txt"), right: MPath("conflict.txt") }]
  [255]
