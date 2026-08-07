# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Content-manifest counterpart of
# test-pushrebase-merge-resolution-no-derive-fsnodes-prederived.t.
#
# Verify that merge resolution works when derive_fsnodes=false and the repo has
# migrated to content manifests (`derived_data_use_content_manifests` on), with
# content manifests already derived.
#
# This is the regression test for the pre-check probing the wrong manifest type:
# `fetch_manifest_file` reads content manifests when the knob is on, so the
# pre-check must probe content manifests too. When it hardcoded
# `fetch_derived::<RootFsnodeId>` it found nothing on a migrated repo and
# silently skipped merge resolution forever, rejecting conflicts that were
# perfectly resolvable. Before that fix this test fails on the final push with
# "Conflicts while pushrebasing".

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig push.edenapi=true

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:derived_data_use_content_manifests": true,
  >     "scm/mononoke:pushrebase_enable_merge_resolution": true,
  >     "scm/mononoke:pushrebase_merge_resolution_derive_fsnodes": false
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

Restart mononoke with WBC derivation disabled
  $ killandwait $MONONOKE_PID
  $ start_and_wait_for_mononoke_server --enable-wbc-with no-derivation

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

Explicitly derive content manifests so the pre-check finds them.
Note we deliberately do NOT derive fsnodes: on a migrated repo they may not be
derived at all, and the pre-check must not depend on them.
  $ mononoke_admin derived-data -R repo derive -T content_manifests --all-bookmarks

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

Derive content manifests for the new bookmark position too
  $ mononoke_admin derived-data -R repo derive -T content_manifests --all-bookmarks

Client commit (from pre-server base): modify the LAST line
  $ hg up -q .~1
  $ cat > shared.txt << 'EOF'
  > line1
  > line2
  > line3
  > line4
  > CLIENT_EDIT_LINE5
  > EOF
  $ hg ci -m "client: edit line 5"

Pushrebase should succeed — content manifests are pre-derived, merge resolution works
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

Verify the merged file has BOTH edits
  $ hg up -q master_bookmark
  $ cat shared.txt
  SERVER_EDIT_LINE1
  line2
  line3
  line4
  CLIENT_EDIT_LINE5
