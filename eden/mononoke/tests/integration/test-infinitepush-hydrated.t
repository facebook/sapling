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

  $ enable amend infinitepush commitcloud remotenames

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
  $ hg ci -m new1
  $ echo more >> newfile
  $ hg ci -m new2
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
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  2 changesets found
  list of changesets:
  895414f853ef689e40c2af5297febe7b5ff47d67
  c5564d074f737edcfef195087eeca32cca42c718
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 3 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 1 advisory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-bundle: 0 parts total
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes

  $ tglogp
  @  2: c5564d074f73 draft 'new2'
  |
  o  1: 895414f853ef draft 'new1'
  |
  o  0: 3903775176ed public 'a'
  

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

-- pull the draft commits with a fully prefetched public parent
-- note the absence of the `b2x:treegroup2` part and the "0 changes to 0 files" wording,
-- indicative of the fact that we return an "unhydrated" commit, expecting to fetch
-- trees and files on the subsequent `update`
  $ hgmn pull -r c5564d074f73 --debug
  pulling from * (glob)
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  sending clienttelemetry command
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  sending lookup command
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  sending getbundle command
  bundle2-input-bundle: 1 params with-transaction
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset 895414f853ef
  add changeset c5564d074f73
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: "phase-heads" supported
  bundle2-input-part: total payload size * (glob)
  bundle2-input-bundle: 1 parts total
  remotenames: skipped syncing local bookmarks

-- update to the recently pulled draft commit
-- note the presence of peer connection, the `gettreepack` and `getpackv1` wireproto commands
-- indicative of actually fetching commit contents
  $ hgmn up -r c5564d074f73 --debug
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
   ancestor: 3903775176ed, local: 3903775176ed+, remote: c5564d074f73
  reusing connection from pool
  sending getpackv1 command
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

stop mononoke before running it with a different config
  $ killandwait "$MONONOKE_PID"
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

-- pull the draft commits with a fully prefetched public parent
-- note the presence of the `b2x:treegroup2` part and the "2 changes to 1 files" wording,
-- indicative of the fact that we return a "hydrated" commit
  $ hgmn pull -r c5564d074f73 --debug
  pulling from ssh://user@dummy/repo
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  sending clienttelemetry command
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  sending lookup command
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  sending getbundle command
  bundle2-input-bundle: 1 params with-transaction
  bundle2-input-part: "changegroup" (params: 1 mandatory) supported
  adding changesets
  add changeset 895414f853ef
  add changeset c5564d074f73
  adding manifests
  adding file changes
  adding newfile revisions
  added 2 changesets with 2 changes to 1 files
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: "b2x:treegroup2" (params: 3 mandatory) supported
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: "phase-heads" supported
  bundle2-input-part: total payload size * (glob)
  bundle2-input-bundle: 2 parts total
  remotenames: skipped syncing local bookmarks

-- update to the draft commit in the middle of the stack
-- note the absence of any wireproto commands
  $ hgmn up -r 895414f853ef --debug
  resolving manifests
   branchmerge: False, force: False, partial: False
   ancestor: 3903775176ed, local: 3903775176ed+, remote: 895414f853ef
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
