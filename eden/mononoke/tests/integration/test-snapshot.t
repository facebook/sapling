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

Make a commit in the first client and upload it
This test also checks file content deduplication. We upload 1 file content and 100 filenodes here.
  $ cd repo
  $ echo b > a
  $ hg snapshot
  abort: you need to specify a subcommand (run with --help to see a list of subcommands)
  [255]
  $ hg snapshot createremote
