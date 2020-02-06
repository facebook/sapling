# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs=
  > [lfs]
  > threshold=20B
  > usercache=$TESTTMP/lfs-cache
  > EOF

  $ LFS_THRESHOLD="20" setup_common_config blob_files
  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo foo > a
  $ echo foo > b
  $ hg addremove && hg ci -m 'initial'
  adding a
  adding b
  $ echo 'bar' > a
  $ hg addremove && hg ci -m 'a => bar'
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke with LFS enabled
  $ mononoke
  $ lfs_uri="$(lfs_server)/repo"
  $ wait_for_mononoke

Make client repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg client-push --noupdate --config extensions.remotenames= -q
  $ cd client-push
  $ setup_hg_client
  $ setup_hg_lfs "$lfs_uri" 1000B "$TESTTMP/lfs-cache1"

Push to Mononoke
  $ cd "$TESTTMP/client-push"
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF
  $ hg up -q tip

  $ LONG="$(yes A 2>/dev/null | head -c 40)"
  $ echo "$LONG" > lfs-largefile
  $ hg commit -Aqm "add lfs-large files"
  $ hgmn push -r . --to master_bookmark -v
  pushing rev b2a5e71d6d8d to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       206 (changelog)
       179  lfs-largefile
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

  $ LONG2="$(yes B 2>/dev/null | head -c 30)"
  $ echo "$LONG2" > lfs-largefile
  $ hg commit -Aqm "modify lfs-large file"
  $ hgmn push -r . --to master_bookmark -v
  pushing rev 0700ec892f3c to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       208 (changelog)
       169  lfs-largefile
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  $ hg mv lfs-largefile lfs-renamed-largefile
  $ hg commit -Aqm "move lfs-large file"
  $ hgmn push -r . --to master_bookmark -v
  pushing rev b75c987b6343 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       228 (changelog)
       251  lfs-renamed-largefile
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Push normal file
  $ echo 1 > smallfile
  $ hg commit -Aqm "normal file"
  $ hgmn push -r . --to master_bookmark -q


Sync it to another client
  $ cd "$TESTTMP/repo-hg"
  $ enable_replay_verification_hook
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > EOF
  $ cd "$TESTTMP"

Sync a lfs pushrebase
  $ mononoke_hg_sync repo-hg 1 --generate-bundles 2>&1 | grep 'successful sync'
  * successful sync of entries [2] (glob)
  $ mononoke_hg_sync repo-hg 2 --generate-bundles 2>&1 | grep 'successful sync'
  * successful sync of entries [3] (glob)
  $ mononoke_hg_sync repo-hg 3 --generate-bundles 2>&1 | grep 'successful sync'
  * successful sync of entries [4] (glob)
  $ mononoke_hg_sync repo-hg 4 --generate-bundles 2>&1 | grep 'successful sync'
  * successful sync of entries [5] (glob)
  $ cd "$TESTTMP/repo-hg"
  $ hg debugfilerev lfs-largefile -v -r 2
  b2a5e71d6d8d: add lfs-large files
   lfs-largefile: bin=1 lnk=0 flag=2000 size=40 copied='' chain=860e3f333d61
    rawdata: 'version https://git-lfs.github.com/spec/v1\noid sha256:c12949887b7d8c46e9fcc5d9cd4bd884de33c1d00e24d7ac56ed9200e07f31a1\nsize 40\n'
  $ hg debugfilerev lfs-largefile -v -r 3
  0700ec892f3c: modify lfs-large file
   lfs-largefile: bin=1 lnk=0 flag=2000 size=30 copied='' chain=82324eb7c94b
    rawdata: 'version https://git-lfs.github.com/spec/v1\noid sha256:3c8bc2369a8a90ce1bd6ceb9883cfada7169dde4abe28d70034edea01c0c9a80\nsize 30\n'
  $ hg debugfilerev lfs-renamed-largefile -v -r 4
  b75c987b6343: move lfs-large file
   lfs-renamed-largefile: bin=1 lnk=0 flag=2000 size=30 copied='lfs-largefile' chain=34b0e9a70540
    rawdata: 'version https://git-lfs.github.com/spec/v1\noid sha256:3c8bc2369a8a90ce1bd6ceb9883cfada7169dde4abe28d70034edea01c0c9a80\nsize 30\nx-hg-copy lfs-largefile\nx-hg-copyrev 82324eb7c94b0000f0eb52d4f1933c3cac636066\n'

Setup another client and update to latest commit from mercurial
  $ hgclone_treemanifest ssh://user@dummy/repo-hg client-pull --noupdate --config extensions.remotenames= -q
  $ cd client-pull
  $ setup_hg_client
  $ setup_hg_lfs "$lfs_uri" 1000B "$TESTTMP/lfs-cache1"

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > getpackversion=2
  > EOF

  $ hg up 2 -v
  resolving manifests
  lfs: downloading c12949887b7d8c46e9fcc5d9cd4bd884de33c1d00e24d7ac56ed9200e07f31a1 (40 bytes)
  lfs: processed: c12949887b7d8c46e9fcc5d9cd4bd884de33c1d00e24d7ac56ed9200e07f31a1
  getting a
  getting b
  getting lfs-largefile
  calling hook update.prefetch: edenscm.hgext.remotefilelog.wcpprefetch
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ wc -c lfs-largefile
  40 lfs-largefile
  $ hg up 3 -v
  resolving manifests
  lfs: downloading 3c8bc2369a8a90ce1bd6ceb9883cfada7169dde4abe28d70034edea01c0c9a80 (30 bytes)
  lfs: processed: 3c8bc2369a8a90ce1bd6ceb9883cfada7169dde4abe28d70034edea01c0c9a80
  getting lfs-largefile
  calling hook update.prefetch: edenscm.hgext.remotefilelog.wcpprefetch
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ wc -c lfs-largefile
  30 lfs-largefile
  $ hg up 4 -v
  resolving manifests
  removing lfs-largefile
  getting lfs-renamed-largefile
  calling hook update.prefetch: edenscm.hgext.remotefilelog.wcpprefetch
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ ls
  a
  b
  lfs-renamed-largefile
  $ hg st --change . -C
  A lfs-renamed-largefile
    lfs-largefile
  R lfs-largefile
  $ wc -c lfs-renamed-largefile
  30 lfs-renamed-largefile
  $ hg up -q 5
  $ ls
  a
  b
  lfs-renamed-largefile
  smallfile
  $ cat smallfile
  1
