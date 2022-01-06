# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ DISALLOW_NON_PUSHREBASE=1 setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg-server
  $ cd repo-hg-server
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
  $ blobimport repo-hg-server/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke

setup the client repo
  $ cd $TESTTMP
  $ hgclone_treemanifest ssh://user@dummy/repo-hg-server client --noupdate --config extensions.remotenames= -q

create new hg commits
  $ cd $TESTTMP/client
  $ hg up -q "min(all())"
  $ echo b > b && hg ci -Am b
  adding b

try doing a non-pushrebase push with the new commits
  $ hgmn push --force ssh://user@dummy/repo
  pushing to ssh://user@dummy/repo
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
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

try doing a pushrebase push with the new commits
  $ hgmn push ssh://user@dummy/repo --config extensions.pushrebase= --config extensions.remotenames= --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
