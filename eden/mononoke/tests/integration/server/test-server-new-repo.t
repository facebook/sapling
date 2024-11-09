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

start mononoke
  $ start_and_wait_for_mononoke_server
setup repo
  $ hg clone -q mono:repo repo
  $ cd repo
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma
  $ hg push -q --to master_bookmark --create

clone from the new repo as well
  $ hg clone -q mono:repo repo-clone

Push with bookmark
  $ cd repo-clone
  $ echo withbook > withbook && hg addremove && hg ci -m withbook
  adding withbook
  $ hg push --to withbook --create
  pushing rev 11f53bbd855a to destination mono:repo bookmark withbook
  searching for changes
  exporting bookmark withbook
  $ hg book --remote
     remote/master_bookmark           0e7ec5675652a04069cbf976a42e45b740f3243c
     remote/withbook                  11f53bbd855ac06521a8895bd57e6ce5f46a9980
