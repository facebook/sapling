# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration
  $ setup_common_config
  $ setconfig pull.use-commit-graph=true
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

setup repo
  $ hginit_treemanifest repo
  $ cd repo
  $ testtool_drawdag -R repo --no-default-files <<'EOF'
  > merge
  >  /|
  > A  B
  > # modify: A "1" "1\n"
  > # modify: B "2" "2\n"
  > # copy: merge "2" "1\n" A "1"
  > # delete: merge "1"
  > # message: A "1"
  > # message: B "2"
  > # message: merge "merge"
  > # bookmark: merge master_bookmark
  > EOF
  A=cb8421187885c79e0faeedd7adde4eb80b1c5b6f9d0cd11e0806f42cf2e4c88b
  B=4958e71d48160073ff185bbf886e3b88ac9586eefc279b618d8fe5cbe7935c0d
  merge=2c1bb37eb324c5b677922b34a6f810bd0743cfca3ec5beab7327b1f7ab724b2e
  $ cd $TESTTMP

start mononoke
  $ mononoke
  $ wait_for_mononoke
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ hg pull -B master_bookmark
  pulling from mono:repo
  $ hg up master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st --change . -C
  A 2
    1
  R 1
