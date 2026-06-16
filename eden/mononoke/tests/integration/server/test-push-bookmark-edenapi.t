# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup repo

  $ testtool_drawdag -R repo << EOF
  > A
  > # bookmark: A master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

start mononoke
  $ start_and_wait_for_mononoke_server

clone the repo. EdenApi is on (realistic prod), but the -B bookmark
killswitch push.edenapi-bookmark is left at its default-off value, so -B
bookmark pushes still go through the legacy wireproto path.
  $ hg clone -q mono:repo repo-push
  $ cd repo-push
  $ setconfig push.edenapi=true

Create a bookmark with -B. This documents the current wireproto behavior:
the push goes to the mono:repo destination and the bookmark is exported over
the wire protocol.
  $ echo foo > foo && hg addremove -q && hg ci -qm foo
  $ hg bookmark foo
  $ hg push -B foo --create
  pushing to mono:repo
  searching for changes
  exporting bookmark foo

Fast-forward the bookmark with -B. Again this goes over wireproto and the
bookmark is updated in place.
  $ echo foo2 > foo && hg ci -qm foo2
  $ hg push -B foo
  pushing to mono:repo
  searching for changes
  updating bookmark foo
