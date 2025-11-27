# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setconfig push.edenapi=true
  $ DISALLOW_NON_PUSHREBASE=1 setup_common_config

  $ cd $TESTTMP

setup repo

  $ testtool_drawdag -R repo << EOF
  > A
  > # bookmark: A master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

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
  remote:     Caused by:
  remote:         0: While resolving Changegroup
  remote:         1: Pure pushes are disallowed in this repo
  abort: unexpected EOL, expected netstring digit
  [255]

try doing a pushrebase push with the new commits
  $ hg push --config extensions.pushrebase= --to master_bookmark
  pushing rev 50d1f117c106 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (20ca2a4749a4, 50d1f117c106] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 50d1f117c106
