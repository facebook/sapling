# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP


setup common configuration for these tests
mononoke  local commit cloud backend
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > snapshot =
  > EOF

setup repo
  $ hginit_treemanifest repo
  $ cd repo
  $ mkcommit "base_commit"
  $ hg log -T '{short(node)}\n'
  8b2dca0c8a72
  $ echo a > a
  $ hg addremove -q
  $ hg commit -m "Add a"

blobimport
  $ cd $TESTTMP
  $ blobimport repo/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke

start edenapi
  $ setup_configerator_configs
  $ start_edenapi_server_no_tls

TEST CASES:

Make some local changes
  $ cd repo
  $ echo b > a
  $ hgedenapi snapshot
  abort: you need to specify a subcommand (run with --help to see a list of subcommands)
  [255]

Create a snapshot.
  $ EDENSCM_LOG=edenapi::client=info hgedenapi snapshot createremote
    INFO edenapi::client: Preparing ephemeral bubble
    INFO edenapi::client: Created bubble 1
    INFO edenapi::client: Requesting lookup for 1 item(s)
    INFO edenapi::client: Received 0 token(s) from the lookup_batch request
    INFO edenapi::client: Requesting upload for */repo/upload/file/content_id/21c519fe0eb401bc97888f270902935f858d0c5361211f892fd26ed9ce127ff9 (glob)
    INFO edenapi::client: Received 1 new token(s) from upload requests
