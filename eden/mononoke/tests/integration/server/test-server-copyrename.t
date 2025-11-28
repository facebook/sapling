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

  $ hginit_treemanifest repo
  $ cd repo
  $ testtool_drawdag -R repo --no-default-files <<EOF
  > A
  > # modify: A "a" "a\n"
  > # modify: A "b" "b\n"
  > # bookmark: A master_bookmark
  > EOF
  A=e93503fcfd7e17a6b75dac01b921dbf2bc9648c86e110bbcf3fd99dd62cc44ad

Import and start mononoke
  $ cd "$TESTTMP"
  $ mononoke
  $ wait_for_mononoke

setup repo-push and repo-pull
  $ hg clone -q mono:repo repo-push --noupdate
  $ hg clone -q mono:repo repo-pull --noupdate
push some files with copy/move files

  $ cd $TESTTMP/repo-push
  $ hg up master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg cp a a_copy
  $ hg mv b b_move
  $ hg addremove && hg ci -q -mb
  $ hg push --to master_bookmark
  pushing rev 0c8370c70e36 to destination mono:repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

pull them

  $ cd $TESTTMP/repo-pull
  $ hg up master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -T '{node}\n'
  f1acf6b30a86b0bba3dbc806d29c910a4e7c245b
  $ hg pull
  pulling from mono:repo
  imported commit graph for 1 commit (1 segment)
  $ hg log -T '{node}\n'
  0c8370c70e3622b7ddf1c0130615586b65e09bc3
  f1acf6b30a86b0bba3dbc806d29c910a4e7c245b

push files that modify copied and moved files

  $ cd $TESTTMP/repo-push
  $ echo "aa" >> a_copy
  $ echo "bb" >> b_move
  $ hg addremove && hg ci -q -mc
  $ hg push --to master_bookmark
  pushing rev c0e2c2487ea1 to destination mono:repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

pull them

  $ cd $TESTTMP/repo-pull
  $ hg log -T '{node}\n'
  0c8370c70e3622b7ddf1c0130615586b65e09bc3
  f1acf6b30a86b0bba3dbc806d29c910a4e7c245b
  $ hg pull
  pulling from mono:repo
  imported commit graph for 1 commit (1 segment)
  $ hg log -T '{node}\n'
  c0e2c2487ea1495342b88fdd4a1a50557ee6c4ab
  0c8370c70e3622b7ddf1c0130615586b65e09bc3
  f1acf6b30a86b0bba3dbc806d29c910a4e7c245b
  $ hg up master_bookmark
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat a_copy
  a
  aa
  $ cat b_move
  b
  bb
