# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config blob_files
  $ cd "$TESTTMP"

Setup repo
  $ hginit_treemanifest repo
  $ cd repo
  $ testtool_drawdag -R repo <<EOF
  > A
  > # modify: A "a" "a\n"
  > # bookmark: A master_bookmark
  > EOF
  A=3fddecdce2d4f7f1c8a02110eea417a78142c3c5fc95eba77d64eb7248e38dd2

Import and start mononoke
  $ cd "$TESTTMP"
  $ mononoke
  $ wait_for_mononoke
  $ lfs_uri="$(lfs_server)/repo"

Setup common client configuration for these tests
  $ cat >> "$HGRCPATH" <<EOF
  > [extensions]
  > amend=
  > commitcloud=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF

setup repo-push and repo-pull
  $ cd "$TESTTMP"
  $ hg clone -q mono:repo repo-push --noupdate
  $ cd "${TESTTMP}/repo-push"
  $ setup_hg_modern_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"

  $ cd "$TESTTMP"
  $ hg clone -q mono:repo repo-pull --noupdate
  $ cd "${TESTTMP}/repo-pull"
  $ setup_hg_modern_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"

Do infinitepush (aka commit cloud) push
  $ cd "${TESTTMP}/repo-push"
  $ hg up tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo new > newfile
  $ yes A 2>/dev/null | head -c 200 > large
  $ hg addremove -q
  $ hg ci -m new
  $ NEW_COMMIT=$(hg log -r . -T '{node}')
  $ hg cloud upload -qr .

Try to pull it
  $ cd "${TESTTMP}/repo-pull"
  $ hg pull -r $NEW_COMMIT
  pulling from mono:repo
  searching for changes
