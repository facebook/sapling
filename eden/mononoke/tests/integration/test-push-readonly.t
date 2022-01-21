# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
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
  $ blobimport repo-hg/.hg repo

setup push source repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2

start mononoke

  $ mononoke
  $ wait_for_mononoke

create new commit in repo2 and check that push fails

  $ cd repo2
  $ mkdir b_dir
  $ echo "new a file content" > a
  $ echo "b file content" > b_dir/b
  $ hg add b_dir/b
  $ hg ci -mb

  $ hgmn push --force --config treemanifest.treeonly=True --debug mononoke://$(mononoke_address)/repo
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  sending hello command
  sending clienttelemetry command
  query 1; heads
  sending batch command
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 1
  1 total queries in 0.0000s
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  1 changesets found
  list of changesets:
  bb0985934a0f8a493887892173b68940ceb40b4f
  sending unbundle command
  bundle2-output-bundle: "HG20", 3 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  remote: * (glob)
  remote:   Error:
  remote:     Repo is marked as read-only: Set by config option
  remote: 
  remote:   Root cause:
  remote:     Repo is marked as read-only: Set by config option
  remote: 
  remote:   Debug context:
  remote:     RepoReadOnly(
  remote:         "Set by config option",
  remote:     )
  abort: unexpected EOL, expected netstring digit
  [255]

Try to bypass the check
  $ hgmn push --force --config treemanifest.treeonly=True mononoke://$(mononoke_address)/repo --pushvars "BYPASS_READONLY=true"
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
