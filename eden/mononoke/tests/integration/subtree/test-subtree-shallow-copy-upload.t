# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Shallow subtree copies (the "copies" key in the subtree extra) are rejected at
upload time unless manifest-altering subtree changes are enabled.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig subtree.use-prod-subtree-key=True
  $ setconfig push.edenapi=true
  $ setup_common_config

  $ testtool_drawdag -R repo --derive-all --no-default-files << EOF
  > A-B
  > # modify: A foo/file1 "aaa\n"
  > # modify: B foo/file1 "bbb\n"
  > # bookmark: B master_bookmark
  > EOF
  A=* (glob)
  B=* (glob)

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo

Create a commit with shallow copy metadata, copying foo from A to bar. The
client can no longer create these, so write the metadata directly.
  $ mkdir bar
  $ echo aaa > bar/file1
  $ hg add -q bar/file1
  $ export A_HG=$(hg log -r 'master_bookmark^' -T '{node}')
  $ hg dbsh << 'EOS'
  > import json, os
  > metadata = [{"copies": [{"from_commit": os.environ["A_HG"], "from_path": "foo", "to_path": "bar"}], "v": 1}]
  > repo.commit("shallow copy", extra={"subtree": json.dumps(metadata, separators=(",", ":"))})
  > EOS
  $ SHALLOW=$(hg log -r . -T '{node}')
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"copies":[{"from_commit":"*","from_path":"foo","to_path":"bar"}],"v":1}] (glob)

With manifest-altering subtree changes disabled (the default), the upload is
rejected with a 400 error.
  $ hg push -r . --to master_bookmark 2>&1 | grep abort
  abort: server responded 400 Bad Request for https://localhost:$LOCAL_PORT/edenapi/repo/upload/changesets: {"message":"invalid changeset upload: invalid request: Changeset * contains shallow subtree copies, which are not supported: use a deep subtree copy instead","request_id":"*"}. Headers: { (glob)

Deep subtree copies are unaffected.
  $ hg goto -q master_bookmark
  $ hg subtree copy -r .^ --from-path foo --to-path baz
  copying foo to baz
  $ hg push -q -r . --to master_bookmark

With manifest-altering subtree changes enabled, the shallow copy is accepted.
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:enable_manifest_altering_subtree_changes": true
  >   }
  > }
  > EOF
  $ killandwait $MONONOKE_PID
  $ start_and_wait_for_mononoke_server
  $ hg push -q -r "$SHALLOW" --to shallow_bookmark --create
  $ mononoke_admin fetch -R repo -B shallow_bookmark | grep SUBTREE
  	 SUBTREE_COPY: bar (from foo @ *) (glob)
