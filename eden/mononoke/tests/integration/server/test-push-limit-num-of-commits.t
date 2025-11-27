# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ UNBUNDLE_COMMIT_LIMIT=2 setup_common_config

  $ cd $TESTTMP

setup repo

  $ testtool_drawdag -R repo << EOF
  > A
  > # bookmark: A master_bookmark
  > # modify: A a "a file content"
  > EOF
  A=d672564be4c568b4d175fb2283de2485ea31cbe1d632ff2a6850b69e2940bad8

start mononoke
  $ start_and_wait_for_mononoke_server

setup push source repo
  $ hg clone -q mono:repo repo2

create new commit in repo2 and check that push fails

  $ cd repo2
  $ echo "1" >> a
  $ hg addremove
  $ hg ci -ma

  $ hg push -r . --to master_bookmark
  pushing rev 36dabb88c248 to destination mono:repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark


  $ echo "1" >> a
  $ hg ci -maa
  $ echo "1" >> a
  $ hg ci -maaa
  $ echo "1" >> a
  $ hg ci -maaaa
  $ hg push -r . --to master_bookmark
  pushing rev 4179bfee0535 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote: 
  remote:     Caused by:
  remote:         0: While resolving Changegroup
  remote:         1: Trying to push too many commits! Limit is 2, tried to push 3
  abort: unexpected EOL, expected netstring digit
  [255]
