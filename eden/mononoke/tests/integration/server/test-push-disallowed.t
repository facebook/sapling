# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 DISALLOW_NON_PUSHREBASE=1 setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma
  $ hg bookmark master_bookmark -r 'tip'

verify content
  $ hg log
  commit:      0e7ec5675652
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
blobimport the repo
  $ cd $TESTTMP
  $ blobimport repo/.hg repo

start mononoke

  $ start_and_wait_for_mononoke_server
setup the client repo
  $ cd $TESTTMP
  $ hg clone -q mono:repo client --noupdate

create new hg commits
  $ cd $TESTTMP/client
  $ hg up -q "min(all())"
  $ echo b > b && hg ci -Am b
  adding b

try doing a non-pushrebase push with the new commits
  $ hg push --force --allow-anon
  pushing to mono:repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote: 
  remote:   Root cause:
  remote:     Pure pushes are disallowed in this repo
  remote: 
  remote:   Caused by:
  remote:     While resolving Changegroup
  remote:   Caused by:
  remote:     Pure pushes are disallowed in this repo
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "bundle2_resolver error",
  remote:         source: Error {
  remote:             context: "While resolving Changegroup",
  remote:             source: "Pure pushes are disallowed in this repo",
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

try doing a pushrebase push with the new commits
  $ hg push --config extensions.pushrebase= --to master_bookmark
  pushing rev 95415a1a54e2 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (0e7ec5675652, 95415a1a54e2] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 95415a1a54e2
