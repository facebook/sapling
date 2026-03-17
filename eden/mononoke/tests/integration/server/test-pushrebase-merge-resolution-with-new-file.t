# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Verify that merge resolution works alongside non-conflicting file changes.
#
# The client modifies a file that conflicts with the server (overlap.txt) and
# also adds a brand-new file (brand_new.txt). Merge resolution handles the
# conflicting path, while the non-conflicting file is rebased normally.
# This tests the common case where a push touches multiple files but only
# some of them overlap with server-side changes.

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
  $ cat > overlap.txt << 'EOF'
  > ov_line1
  > ov_line2
  > ov_line3
  > EOF
  $ hg add overlap.txt
  $ hg ci -m "add overlap.txt"
  $ hg push -r . --to master_bookmark -q

Server edits overlap.txt (line 1)
  $ hg up -q master_bookmark
  $ cat > overlap.txt << 'EOF'
  > SERVER_ov_line1
  > ov_line2
  > ov_line3
  > EOF
  $ hg ci -m "server: edit overlap.txt"
  $ hg push -r . --to master_bookmark -q

Client edits overlap.txt (line 3, non-overlapping) AND adds a new file
  $ hg up -q .~1
  $ cat > overlap.txt << 'EOF'
  > ov_line1
  > ov_line2
  > CLIENT_ov_line3
  > EOF
  $ echo "new content" > brand_new.txt
  $ hg add brand_new.txt
  $ hg ci -m "client: edit overlap.txt and add new file"

Pushrebase should succeed — overlap.txt is merge-resolved, brand_new.txt is clean
  $ hg push -r . --to master_bookmark
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue * for upload (glob)
  edenapi: uploaded * (glob)
  edenapi: queue * for upload (glob)
  edenapi: uploaded * (glob)
  edenapi: uploaded 1 changeset
  pushrebasing stack (*, *] (1 commit) to remote bookmark master_bookmark (glob)
  * files updated, 0 files merged, 0 files removed, 0 files unresolved (glob)
  updated remote bookmark master_bookmark to * (glob)

Verify merge-resolved file has both edits
  $ hg up -q master_bookmark
  $ cat overlap.txt
  SERVER_ov_line1
  ov_line2
  CLIENT_ov_line3

Verify the non-conflicting new file was rebased normally
  $ cat brand_new.txt
  new content
