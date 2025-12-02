# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Tests wether we can init a new repo and push/pull to Mononoke, specifically
without blobimport. That validates that we can provision new repositories
without extra work.
  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup repo
  $ testtool_drawdag -R repo <<EOF
  > A
  > # bookmark: A master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

start mononoke
  $ start_and_wait_for_mononoke_server

clone from the new repo as well
  $ hg clone -q mono:repo repo-clone

Push with bookmark
  $ cd repo-clone
  $ echo withbook > withbook && hg addremove && hg ci -m withbook
  adding withbook
  $ hg push --to withbook --create
  pushing rev cdbb2b8b2cf1 to destination mono:repo bookmark withbook
  searching for changes
  exporting bookmark withbook
  $ hg book --remote
     remote/master_bookmark           20ca2a4749a439b459125ef0f6a4f26e88ee7538
     remote/withbook                  cdbb2b8b2cf1612cd6a1271c96a7a89d98b36dd4
