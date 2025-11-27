# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup configuration
  $ INFINITEPUSH_ALLOW_WRITES=true setup_common_config blob_files
  $ cd "$TESTTMP"

Setup repo
  $ hginit_treemanifest repo
  $ cd repo

Create commits using testtool drawdag
  $ testtool_drawdag -R repo --no-default-files <<'EOF'
  > A
  > # modify: A "a" "a\n"
  > # bookmark: A master_bookmark
  > EOF
  A=a420a36db20fa79a604ce354128048c7bdb7c25b881dfe71d37b7443f458a3f0

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
  > EOF

setup repo-push and repo-pull
  $ cd "$TESTTMP"
  $ hg clone -q mono:repo repo-push --noupdate
  $ cd "${TESTTMP}/repo-push"
  $ setup_hg_modern_lfs "$lfs_uri" 10B
  $ setconfig "remotefilelog.cachepath=$TESTTMP/cachepath-push"

  $ cd "$TESTTMP"
  $ hg clone -q mono:repo repo-pull --noupdate
  $ cd "${TESTTMP}/repo-pull"
  $ setup_hg_modern_lfs "$lfs_uri" 10B
  $ setconfig "remotefilelog.cachepath=$TESTTMP/cachepath-pull"

Do infinitepush (aka commit cloud) push

  $ cd "${TESTTMP}/repo-push"
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo new > newfile
  $ yes A 2>/dev/null | head -c 200 > large
  $ hg addremove -q
  $ hg ci -m new
  $ NEW_COMMIT=$(hg log -r . -T '{node}')
  $ hg cloud backup
  commitcloud: head '5339cb7b16c1' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset

Try to pull it
  $ cd "${TESTTMP}/repo-pull"
  $ hg pull -r $NEW_COMMIT
  pulling from mono:repo
  searching for changes
