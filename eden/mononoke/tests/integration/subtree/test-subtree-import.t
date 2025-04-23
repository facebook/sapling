# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Override subtree key to enable non-test subtree extra
  $ cat > $TESTTMP/subtree.py <<EOF
  > from sapling.utils import subtreeutil
  > def extsetup(ui):
  >     subtreeutil.SUBTREE_KEY = "subtree"
  > EOF
  $ setconfig extensions.subtreetestoverride=$TESTTMP/subtree.py
  $ setconfig push.edenapi=true
  $ setconfig subtree.min-path-depth=1
  $ enable amend
  $ setup_common_config

  $ cd $TESTTMP
  $ git init -q gitrepo
  $ cd gitrepo
  $ git config core.autocrlf false
  $ echo 1 > alpha
  $ git add alpha
  $ git commit -q -malpha
  $ mkdir dir1
  $ echo 2 > dir1/beta
  $ git add dir1/beta
  $ git commit -q -mbeta
  $ mkdir dir2
  $ echo 3 > dir2/gamma
  $ git add dir2/gamma
  $ git commit -q -mgamma
  $ cd $TESTTMP
  $ export GIT_URL=git+file://$TESTTMP/gitrepo

  $ testtool_drawdag -R repo --derive-all --no-default-files << EOF
  > A-B-C
  > # modify: A foo/file1 "aaa\n"
  > # modify: A foo/file3 "xxx\n"
  > # copy: B foo/file2 "bbb\n" A foo/file1
  > # delete: B foo/file1
  > # modify: C foo/file2 "ccc\n"
  > # bookmark: C master_bookmark
  > EOF
  A=bad79679db57d8ca7bdcb80d082d1508f33ca2989652922e2e01b55fb3c27f6a
  B=170dbba760afb7ec239d859e2412a827dd7229cdbdfcd549b7138b2451afad37
  C=e611f471e1f2bd488fee752800983cdbfd38d50247e5d81222e0d620fd2a6120

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo

  $ hg subtree import --url $GIT_URL --rev eb6ca104a156be9d0c4dd444cfb41d98dc79d7c7 --to-path bar -m "import gitrepo to bar"
  creating git repo at $TESTTMP/cachepath/gitrepos/* (glob)
  From file://$TESTTMP/gitrepo
   * [new ref]         eb6ca104a156be9d0c4dd444cfb41d98dc79d7c7 -> remote/master_bookmark
   * [new ref]         eb6ca104a156be9d0c4dd444cfb41d98dc79d7c7 -> refs/visibleheads/eb6ca104a156be9d0c4dd444cfb41d98dc79d7c7
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  copying / to bar

  $ hg push -r . --to master_bookmark
  pushing rev * to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue 3 files for upload
  edenapi: uploaded 3 files
  edenapi: queue 4 trees for upload
  edenapi: uploaded 4 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (3527857ec5dd, *] (1 commit) to remote bookmark master_bookmark (glob)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to * (glob)
