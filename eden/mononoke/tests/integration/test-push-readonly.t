# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ REPONAME="test/repo"
  $ configure modern
  $ export READ_ONLY_REPO=1
  $ setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup master bookmarks

  $ hg bookmark master_bookmark -r 'tip'

verify content
  $ hg log
  commit:      0e7ec5675652
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg $REPONAME
  warning: failed to inspect working copy parent

setup push source repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2

start mononoke

  $ start_and_wait_for_mononoke_server
create new commit in repo2 and check that push to a bookmark fails

  $ cd repo2
  $ mkdir b_dir
  $ echo "new a file content" > a
  $ echo "b file content" > b_dir/b
  $ hg add b_dir/b
  $ hg ci -mb

  $ hgmn push --to master_bookmark --force --config treemanifest.treeonly=True --debug mononoke://$(mononoke_address)/test%2Frepo
  sending hello command
  sending clienttelemetry command
  pushing rev bb0985934a0f to destination mononoke://$LOCALIP:$LOCAL_PORT/test/repo bookmark master_bookmark
  query 1; heads
  preparing listkeys for "bookmarks" with pattern "['master']"
  sending batch command
  received listkey for "bookmarks": 0 bytes
  sending heads command
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 1
  1 total queries in 0.0000s
  preparing listkeys for "bookmarks" with pattern "['master_bookmark']"
  sending listkeyspatterns command
  received listkey for "bookmarks": 56 bytes
  1 changesets found
  list of changesets:
  bb0985934a0f8a493887892173b68940ceb40b4f
  sending unbundle command
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  remote: * (glob)
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:   Root cause:
  remote:     Repo is locked: Set by config option
  remote: 
  remote:   Caused by:
  remote:     Failed to fast-forward bookmark (set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)
  remote:   Caused by:
  remote:     Repo is locked: Set by config option
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a push",
  remote:         source: Error {
  remote:             context: "Failed to fast-forward bookmark (set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)",
  remote:             source: RepoLocked(
  remote:                 "Set by config option",
  remote:             ),
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

Try to bypass the check
  $ hgmn push --force --to master_bookmark --config treemanifest.treeonly=True mononoke://$(mononoke_address)/test%2Frepo --pushvars "BYPASS_READONLY=true"
  pushing rev bb0985934a0f to destination mononoke://$LOCALIP:$LOCAL_PORT/test/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

Check that a push which doesn't move a bookmark is allowed
  $ hgmn push --force --config treemanifest.treeonly=True --debug mononoke://$(mononoke_address)/test%2Frepo
  tracking on None {}
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/test/repo
  sending hello command
  sending clienttelemetry command
  query 1; heads
  preparing listkeys for "bookmarks" with pattern "['master']"
  sending batch command
  received listkey for "bookmarks": 0 bytes
  sending heads command
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 1
  all local heads known remotely
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  sending known command
  no changes found
  preparing listkeys for "bookmarks" with pattern "['master']"
  sending listkeyspatterns command
  received listkey for "bookmarks": 0 bytes
  [1]
