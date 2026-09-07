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

Scenario 2 -- changeset-upload derivation OFF, everything else identical, and the
two tree-upload knobs this stack adds pinned ON so that later diffs change what
this one upload produces rather than adding uploads of their own. Nothing reads
those two yet.

They are declared here rather than in the header on purpose. Scenario 1 has to
upload with them off: with them on, its tree upload would write envelopes before
the changeset derivation ran, and puts are if-absent, so the derivation's own put
would silently become a no-op.

The running server picks up the new values without a restart.
  $ merge_just_knobs <<EOF
  > {"bools": {"scm/mononoke:derive_hg_augmented_manifest_with_hg_changeset": false, "scm/mononoke:build_augmented_manifests_at_tree_upload": true, "scm/mononoke:store_augmented_manifests_at_tree_upload": true}}
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
  $ ROOT_MFID_2=$(sl log -r . -T '{manifest}')
  $ cd $TESTTMP
  $ mononoke_admin derived-data -R repo exists -T hgchangesets -i "$CS2"
  Derived: * (glob)
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests -i "$CS2"
  Not Derived: * (glob)

Everything below probes that one upload, and the later diffs in this stack change
what it records instead of adding scenarios of their own. The two lines above are
the control and never change: no augmented manifest is ever derived for this
commit, so anything that does appear is attributable to the tree-upload path
rather than to anything else in the upload.

First, whether an envelope exists at the root tree's key. Deriving is not the only
way one could appear, since the tree upload stores blobs of its own. The key is
the hg manifest id alone -- no changeset, no mapping row -- which is the same
lookup the serve path does, and the reason an envelope built outside per-changeset
derivation is servable at all. `fetch-many` rather than `fetch` because it always
prints its three counts, so a hit changes a recorded number rather than deleting a
line.
  $ echo "hgaugmentedmanifest.sha1.$ROOT_MFID_2" > envelope_keys
  $ mononoke_admin blobstore -R repo fetch-many --keys-file envelope_keys
  present: 0
  missing: 1
  failed: 0

Second, what the trees endpoint serves for that same tree. All four attributes are
spelled out because `TreeAttributes` derives `#[serde(default)]` and `parents` and
`child_metadata` default to true, so omitting them would ask for metadata here and
make the recorded values move for a reason unrelated to the route.
  $ cat > tree_attrs << EOF
  > {
  >     "manifest_blob": True,
  >     "parents": False,
  >     "child_metadata": False,
  >     "augmented_trees": False
  > }
  > EOF
  $ cat > tree_keys << EOF
  > [
  >     ("", "$ROOT_MFID_2")
  > ]
  > EOF

Routing is off, so this is the original manifest served the way it is served
today. `manifest_blob_sha1` is the one value that must survive routing being
turned on: the augmented path stores no copy of these bytes, it rebuilds them by
re-serialising the augmented subentries back into legacy manifest lines, so that
hash holding still while the rest of the block moves is what shows the round trip
is faithful.
  $ hg debugapi mono:repo -e trees -f tree_keys -f tree_attrs --sort > "$TESTTMP/served.out" 2>&1
  $ python3 -c "
  > import hashlib
  > bin = lambda x: x
  > e = eval(open('$TESTTMP/served.out').read())[0]
  > print('manifest_blob_sha1=%s' % hashlib.sha1(e['data']).hexdigest())
  > print('tree_aux_data=%s' % (e.get('tree_aux_data') is not None))
  > print('has_acl=%s' % e.get('has_acl'))
  > print('parents_present=%s' % (e.get('parents') is not None))
  > print('children=%s' % len(e.get('children') or []))
  > "
  manifest_blob_sha1=a06e8b2b61feaa5b804299db3dd2f707ff6bd8ae
  tree_aux_data=False
  has_acl=None
  parents_present=False
  children=0
