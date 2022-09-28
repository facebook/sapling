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
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg addremove && hg ci -q -ma
  adding a
  $ hg log -T '{short(node)}\n'
  3903775176ed
  $ hg bookmark master_bookmark -r tip

  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo

Start Mononoke
  $ start_and_wait_for_mononoke_server
  $ lfs_uri="$(lfs_server)/repo"

Setup common client configuration for these tests
  $ cat >> "$HGRCPATH" <<EOF
  > [extensions]
  > amend=
  > infinitepush=
  > commitcloud=
  > remotenames=
  > [infinitepush]
  > server=False
  > EOF

setup repo-push and repo-pull
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ cd "${TESTTMP}/repo-push"
  $ setup_hg_modern_lfs "$lfs_uri" 10B
  $ setconfig "remotefilelog.cachepath=$TESTTMP/cachepath-push"

  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate
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
  $ hgmn cloud backup
  backing up stack rooted at 68394cf51f7e
  commitcloud: backed up 1 commit

Try to pull it
  $ cd "${TESTTMP}/repo-pull"
  $ hgmn pull -r 68394cf51f7e96952fe832a3c05d17a9b49e8b4b
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
