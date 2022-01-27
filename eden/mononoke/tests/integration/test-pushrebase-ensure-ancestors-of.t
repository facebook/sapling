# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ DISALLOW_NON_PUSHREBASE=1 EMIT_OBSMARKERS=1 setup_common_config "blob_files"
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

  $ newrepo repo-hg
  $ setup_hg_server
  $ hg debugdrawdag <<EOF
  > B
  > |
  > A
  > EOF

- Create master_bookmark 

  $ hg bookmark master_bookmark -r B

- Import and start Mononoke (the Mononoke repo name is 'repo')

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo
  $ start_and_wait_for_mononoke_server
Prepare the client-side repo

  $ hgclone_treemanifest ssh://user@dummy/repo-hg client-repo --noupdate --config extensions.remotenames= -q
  $ cd $TESTTMP/client-repo
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

Push commit to ancestor bookmark, should work
  $ hgmn up -q master_bookmark
  $ hgmn push -r . --to ancestor --create
  pushing rev 112478962961 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark ancestor
  searching for changes
  no changes found
  exporting bookmark ancestor

Now try to pushrebase "ancestor" bookmark, should fail
  $ touch file
  $ hg addremove -q
  $ hg ci -m 'new commit'
  $ hgmn push -r . --to ancestor
  pushing rev 9ddef2ba352e to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark ancestor
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Pushrebase is not allowed onto the bookmark 'ancestor', because this bookmark is required to be an ancestor of 'master_bookmark'
  remote: 
  remote:   Root cause:
  remote:     Pushrebase is not allowed onto the bookmark 'ancestor', because this bookmark is required to be an ancestor of 'master_bookmark'
  remote: 
  remote:   Debug context:
  remote:     PushrebaseNotAllowedRequiresAncestorsOf {
  remote:         bookmark: BookmarkName {
  remote:             bookmark: "ancestor",
  remote:         },
  remote:         descendant_bookmark: BookmarkName {
  remote:             bookmark: "master_bookmark",
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

Now push this commit to another bookmark
  $ hgmn push -r . --to another_bookmark --create
  pushing rev 9ddef2ba352e to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark another_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  exporting bookmark another_bookmark

And try to move "ancestor" bookmark there, it should fail
  $ hgmn push -r . --to ancestor
  pushing rev 9ddef2ba352e to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark ancestor
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a bookmark-only pushrebase
  remote: 
  remote:   Root cause:
  remote:     Bookmark 'ancestor' can only be moved to ancestors of 'master_bookmark'
  remote: 
  remote:   Caused by:
  remote:     Failed to fast-forward bookmark (set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)
  remote:   Caused by:
  remote:     Bookmark 'ancestor' can only be moved to ancestors of 'master_bookmark'
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a bookmark-only pushrebase",
  remote:         source: Error {
  remote:             context: "Failed to fast-forward bookmark (set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)",
  remote:             source: RequiresAncestorOf {
  remote:                 bookmark: BookmarkName {
  remote:                     bookmark: "ancestor",
  remote:                 },
  remote:                 descendant_bookmark: BookmarkName {
  remote:                     bookmark: "master_bookmark",
  remote:                 },
  remote:             },
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

