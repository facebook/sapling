# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

setup
  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup repo
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup data
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

setup client repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-client
  $ cd repo-client
  $ setup_hg_client

start mononoke
  $ mononoke
  $ wait_for_mononoke

pull from mononoke and log data
  $ MOCK_USERNAME=foobar CLIENT_DEBUG=true LOCALIP="127.0.0.1" hgmn pull
  pulling from ssh://user@dummy/repo
  remote: Metadata {
  remote:     session_id: SessionId(
  remote:         "*", (glob)
  remote:     ),
  remote:     identities: {
  remote:         MononokeIdentity {
  remote:             id_type: "USER",
  remote:             id_data: "foobar",
  remote:         },
  remote:     },
  remote:     priority: Default,
  remote:     client_debug: true,
  remote:     client_ip: Some(
  remote:         V4(
  remote:             $LOCALIP,
  remote:         ),
  remote:     ),
  remote:     client_hostname: Some(
  remote:         "localhost",
  remote:     ),
  remote: }
  searching for changes
  no changes found

