# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
  $ cd $TESTTMP

setup common configuration for these tests
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend=
  > infinitepush=
  > commitcloud=
  > EOF

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg addremove && hg ci -q -ma
  adding a
  $ hg log -T '{short(node)}\n'
  3903775176ed

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-push and repo-pull
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull-unhydrated --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull-hydrated --noupdate

blobimport

  $ blobimport repo-hg/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke

Do infinitepush (aka commit cloud) push
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo new > newfile
  $ hg addremove -q
  $ hg ci -m new
  $ hgmn push ssh://user@dummy/repo -r . --bundle-store --debug --allow-anon
  pushing to ssh://user@dummy/repo
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  1 changesets found
  list of changesets:
  47da8b81097c5534f3eb7947a8764dd323cffe3d
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 3 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 1 advisory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-bundle: 0 parts total
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes

  $ tglogp
  @  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

check unhydrated infinitepush pulls
  $ cd "$TESTTMP/repo-pull-unhydrated"

-- do a public pull.
  $ hgmn pull |& grep "changesets"
  adding changesets
  added * changesets with 0 changes to 0 files (glob)
  $ tglogpnr -r "draft()"

-- update to a public parent of the susequently pulled draft commit
-- so that prefetchdraftparents does not cause a `gettreepack`
  $ hgmn up -q 3903775176ed

-- pull a draft commit with a fully prefetched public parent
-- note the absense of the `b2x:treegroup2` part and the "0 changes to 0 files" wording,
-- indicative of the fact that we return an "unhydrated" commit, expecting to fetch
-- trees and files on the subsequent `update`
  $ hgmn pull -r 47da8b81097c --debug
  pulling from * (glob)
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  sending clienttelemetry command
  sending lookup command
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  sending getbundle command
  bundle2-input-bundle: 1 params with-transaction
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset 47da8b81097c
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: "phase-heads" supported
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: "listkeys" (params: 1 mandatory) supported
  bundle2-input-part: total payload size * (glob)
  bundle2-input-bundle: 2 parts total
  checking for updated bookmarks

-- update to the recently pullued draft commit
-- note the presence of peer connection, the `gettreepack` and `getpackv1` wireproto commands
-- indicative of actually fetching commit contents
  $ hgmn up -r 47da8b81097c --debug
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  sending clienttelemetry command
  sending gettreepack command
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "b2x:treegroup2" (params: 3 mandatory) supported
  bundle2-input-part: total payload size * (glob)
  bundle2-input-bundle: 0 parts total
  resolving manifests
   branchmerge: False, force: False, partial: False
   ancestor: 3903775176ed, local: 3903775176ed+, remote: 47da8b81097c
  reusing connection from pool
  sending getpackv1 command
   newfile: remote created -> g
  getting newfile
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

stop mononoke before running it with a different config
  $ kill "$MONONOKE_PID"
  $ rm -rf "$TESTTMP/mononoke-config"

setup a new config and restart mononoke
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' INFINITEPUSH_HYDRATE_GETBUNDLE_RESPONSE=true setup_common_config
  $ mononoke
  $ wait_for_mononoke

check hydrated infinitepush pulls
  $ cd "$TESTTMP/repo-pull-hydrated"

-- do a public pull.
  $ hgmn pull |& grep "changesets"
  adding changesets
  added * changesets with 0 changes to 0 files (glob)
  $ tglogpnr -r "draft()"

-- update to a public parent of the susequently pulled draft commit
-- so that prefetchdraftparents does not cause a `gettreepack`
  $ hgmn up -q 3903775176ed

-- pull a draft commit with a fully prefetched public parent
-- note the presence of the `b2x:treegroup2` part and the "1 changes to 1 files" wording,
-- indicative of the fact that we return a "hydrated" commit
  $ hgmn pull -r 47da8b81097c --debug
  pulling from ssh://user@dummy/repo
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  sending clienttelemetry command
  sending lookup command
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  sending getbundle command
  bundle2-input-bundle: 1 params with-transaction
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset 47da8b81097c
  adding manifests
  adding file changes
  adding newfile revisions
  added 1 changesets with 1 changes to 1 files
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: "b2x:treegroup2" (params: 3 mandatory) supported
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: "phase-heads" supported
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: "listkeys" (params: 1 mandatory) supported
  bundle2-input-part: total payload size * (glob)
  bundle2-input-bundle: 3 parts total
  checking for updated bookmarks

-- update to the recently pullued draft commit
-- note the absense of any wireproto commands
  $ hgmn up -r 47da8b81097c --debug
  resolving manifests
   branchmerge: False, force: False, partial: False
   ancestor: 3903775176ed, local: 3903775176ed+, remote: 47da8b81097c
   newfile: remote created -> g
  getting newfile
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
