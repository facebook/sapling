# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Baseline: which upload endpoint derives what, on the commit-cloud path today.

Cloud upload rather than push, so attribution is unambiguous: push also runs
pushrebase, which reaches a second call site gated by the same knob.

Attribution comes from scuba rows tagged `Generated derived data batch`, which
carry the `http_path` of the driving request. `edenapi_method` is null on those
rows -- it is only stamped on the terminal `EdenAPI Request Processed` row.

  $ . "${TEST_FIXTURES}/library.sh"

  $ configure modern
  $ INFINITEPUSH_ALLOW_WRITES=true setup_common_config
  $ cd $TESTTMP

Commit cloud client config, copied from server/test-commitcloud-upload.t. The
`local` service type keeps the workspace state in $TESTTMP, so no commit cloud
backend is needed.
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend =
  > commitcloud =
  > [commitcloud]
  > hostname = testhost
  > servicetype = local
  > servicelocation = $TESTTMP
  > owner_team = The Test Team
  > [visibility]
  > enabled = True
  > [mutation]
  > record = True
  > enabled = True
  > date = 0 0
  > [remotefilelog]
  > reponame=repo
  > EOF

The other two augmented-manifest knobs are pinned OFF: on-demand would derive at
serve time and steal the attribution, and route-to-augmented would fail closed
on the missing manifest the knob-OFF scenario creates.
  $ merge_just_knobs <<EOF
  > {"bools": {"scm/mononoke:derive_hg_augmented_manifest_with_hg_changeset": true, "scm/mononoke:derive_hg_augmented_manifest_on_demand": false, "scm/mononoke:route_original_to_augmented_hg_manifest": false}}
  > EOF

`--no-derive-hg-augmented` stops the fixture pre-deriving, so every derivation
below belongs to the upload path.
  $ quiet testtool_drawdag -R repo --no-derive-hg-augmented <<EOF
  > A
  > # bookmark: A master_bookmark
  > EOF

`--scuba-log-file` redirects the server request scuba to a file the test can
read; this is the same mechanism test-edenapi-server-files.t uses.
  $ SCUBA="$TESTTMP/scuba.json"
  $ start_and_wait_for_mononoke_server --scuba-log-file "$SCUBA"

  $ sl clone -q mono:repo client1
  $ cd client1
  $ sl goto master_bookmark -q
  $ sl cloud join -q

`derivations_since` prints `<endpoint> <derived_data_type>` for every derivation
that COMPLETED after the given scuba row offset, so each scenario inspects only
its own rows. The changeset ids are stripped off the message to keep the output
hash-free. `wait_for_upload` first blocks until the changeset-upload request has
been logged; scuba rows are written as the request runs and the terminal row is
last, so its presence means every derivation row for that request has landed.
  $ scuba_rows() { wc -l < "$SCUBA"; }
  $ wait_for_upload() {
  >   for _ in $(seq 1 150); do
  >     tail -n +$(($1 + 1)) "$SCUBA" \
  >       | jq -e 'select(.normal.log_tag == "EdenAPI Request Processed"
  >                       and .normal.edenapi_method == "upload_hg_changesets")' \
  >       > /dev/null 2>&1 && return 0
  >     sleep 0.1
  >   done
  >   echo "timed out waiting for upload_hg_changesets request row" >&2
  > }
  $ derivations_since() {
  >   tail -n +$(($1 + 1)) "$SCUBA" \
  >     | jq -r 'select(.normal.log_tag == "Generated derived data batch")
  >              | "\(.normal.http_path // "<none>") \(.normal.msg | split(" ")[0])"' \
  >     | sort -u
  > }

Scenario 1 -- knob ON. Nested directory so there is more than one tree.
  $ BEFORE=$(scuba_rows)
  $ mkdir -p dir
  $ echo one > dir/file
  $ sl commit -qAm "commit with a nested dir"
  $ sl cloud upload
  commitcloud: head '*' hasn't been uploaded yet (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset

Trees upload before the changeset -- the ordering the future change depends on.

Everything is derived under /repo/upload/changesets; /repo/upload/trees derives
nothing. Moving the work means these rows should move to /repo/upload/trees.
  $ wait_for_upload "$BEFORE"
  $ derivations_since "$BEFORE"
  /repo/upload/changesets acl_manifests
  /repo/upload/changesets hg_augmented_manifests

  $ derivations_since "$BEFORE" | grep -c '^/repo/upload/trees ' || true
  0

End state agrees. hgchangesets is present but absent from the rows above: the
upload stores the client-supplied changeset, nothing derives it.
  $ CS1=$(sl log -r . -T '{node}')
  $ cd $TESTTMP
  $ mononoke_admin derived-data -R repo exists -T hgchangesets -i "$CS1"
  Derived: * (glob)
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests -i "$CS1"
  Derived: * (glob)

Scenario 2 -- knob OFF, everything else identical. The running server picks up
the new value without a restart.
  $ merge_just_knobs <<EOF
  > {"bools": {"scm/mononoke:derive_hg_augmented_manifest_with_hg_changeset": false}}
  > EOF
  $ force_update_configerator

  $ cd "$TESTTMP/client1"
  $ BEFORE=$(scuba_rows)
  $ echo two > dir/file2
  $ sl commit -qAm "second commit with a nested dir"
  $ sl cloud upload
  commitcloud: head '*' hasn't been uploaded yet (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset

Same upload, same endpoints, nothing derived. The knob is what removed the work.
  $ wait_for_upload "$BEFORE"
  $ derivations_since "$BEFORE"

  $ CS2=$(sl log -r . -T '{node}')
  $ cd $TESTTMP
  $ mononoke_admin derived-data -R repo exists -T hgchangesets -i "$CS2"
  Derived: * (glob)
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests -i "$CS2"
  Not Derived: * (glob)
