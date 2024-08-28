# Copyright (c) Meta Platforms, Inc. and affiliates.
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
  $ hginit_treemanifest repo
  $ cd repo
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

create master bookmark
  $ hg bookmark master_bookmark -r tip

setup data
  $ cd $TESTTMP
  $ blobimport repo/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
setup config
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > clienttelemetry=
  > [clienttelemetry]
  > announceremotehostname=true
  > EOF

set up the local repo
  $ hg clone -q ssh://user@dummy/repo local
  $ cd local
  $ hgmn pull
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  connected to * (glob)
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes
  $ hgmn pull -q
  $ hgmn pull --config clienttelemetry.announceremotehostname=False
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes
