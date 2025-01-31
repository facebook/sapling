# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ configure modern
  $ cat >> "$ACL_FILE" << ACLS
  > {
  >   "repos": {
  >     "repo": {
  >       "actions": {
  >         "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"],
  >         "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"],
  >         "bypass_readonly": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"]
  >       }
  >     }
  >   }
  > }
  > ACLS
  $ READ_ONLY_REPO=1 ACL_NAME="repo" setup_common_config

  $ cd $TESTTMP

setup repo

  $ testtool_drawdag -R repo << EOF
  > A
  > # bookmark: A master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

start mononoke
  $ start_and_wait_for_mononoke_server


setup push source repo
  $ hg clone -q mono:repo repo2


create new commit in repo2 and check that push to a bookmark fails

  $ cd repo2
  $ mkdir b_dir
  $ echo "new a file content" > a
  $ echo "b file content" > b_dir/b
  $ hg add b_dir/b
  $ hg ci -mb

  $ hg push --to master_bookmark --force --debug
  sending hello command
  sending clienttelemetry command
  pushing rev 4bdc98495893 to destination mono:repo bookmark master_bookmark
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 1
  1 total queries in 0.0000s
  preparing listkeys for "bookmarks" with pattern "['master_bookmark']"
  sending listkeyspatterns command
  received listkey for "bookmarks": 56 bytes
  1 changesets found
  list of changesets:
  4bdc9849589377925a3c7b0f1e72f4c4f7adfb87
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
  remote:     Caused by:
  remote:         0: Failed to fast-forward bookmark (set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)
  remote:         1: Repo is locked: Set by config option
  abort: unexpected EOL, expected netstring digit
  [255]

Try to bypass the check
  $ hg push --force --to master_bookmark --pushvars "BYPASS_READONLY=true"
  pushing rev 4bdc98495893 to destination mono:repo bookmark master_bookmark
  searching for changes
  no changes found
  updating bookmark master_bookmark

Check that a push which doesn't move a bookmark is allowed
  $ hg push --force --debug
  tracking on None {}
  pushing to mono:repo
  sending hello command
  sending clienttelemetry command
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 1
  all local heads known remotely
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  sending known command
  no changes found
  preparing listkeys for "bookmarks" with pattern "['master_bookmark']"
  sending listkeyspatterns command
  received listkey for "bookmarks": 56 bytes
  [1]
