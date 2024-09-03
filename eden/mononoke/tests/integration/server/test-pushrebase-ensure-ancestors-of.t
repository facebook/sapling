# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 DISALLOW_NON_PUSHREBASE=1 EMIT_OBSMARKERS=1 setup_common_config "blob_files"
  $ cat >> repos/repo/server.toml << EOF
  > [[bookmarks]]
  > name="ancestor"
  > ensure_ancestor_of="master_bookmark"
  > EOF
  $ cd $TESTTMP

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > pushrebase =
  > EOF

Prepare the server-side repo

  $ hginit_treemanifest repo
  $ cd repo
  $ hg debugdrawdag <<EOF
  > B
  > |
  > A
  > EOF

- Create master_bookmark 

  $ hg bookmark master_bookmark -r B

- Import and start Mononoke (the Mononoke repo name is 'repo')

  $ cd $TESTTMP
  $ blobimport repo/.hg repo
  $ start_and_wait_for_mononoke_server
Prepare the client-side repo

  $ hg clone -q mono:repo client-repo --noupdate
  $ cd $TESTTMP/client-repo
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

Push commit to ancestor bookmark, should work
  $ hg up -q master_bookmark
  $ hg push -r . --to ancestor --create
  pushing rev 112478962961 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark ancestor
  creating remote bookmark ancestor

Now try to pushrebase "ancestor" bookmark, should fail
  $ touch file
  $ hg addremove -q
  $ hg ci -m 'new commit'
  $ hg push -r . --to ancestor
  pushing rev 9ddef2ba352e to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark ancestor
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (112478962961, 9ddef2ba352e] (1 commit) to remote bookmark ancestor
  abort: Server error: invalid request: Pushrebase is not allowed onto the bookmark 'ancestor', because this bookmark is required to be an ancestor of 'master_bookmark'
  [255]

Now push this commit to another bookmark
  $ hg push -r . --to another_bookmark --create
  pushing rev 9ddef2ba352e to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark another_bookmark
  creating remote bookmark another_bookmark

And try to move "ancestor" bookmark there, it should fail
  $ hg push -r . --to ancestor
  pushing rev 9ddef2ba352e to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark ancestor
  moving remote bookmark ancestor from 112478962961 to 9ddef2ba352e
  abort: server error: invalid request: Bookmark 'ancestor' can only be moved to ancestors of 'master_bookmark'
  [255]
