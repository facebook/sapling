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
  $ testtool_drawdag -R repo << EOF
  > A
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

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
  $ hg clone -q mono:repo local
  $ cd local
  $ hg pull
  pulling from mono:repo
  $ hg pull -q
  $ hg pull --config clienttelemetry.announceremotehostname=False
  pulling from mono:repo
