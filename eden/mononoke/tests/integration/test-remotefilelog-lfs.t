# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

Setup repo config (we use blob_files to share across Mononoke and API Server):
  $ LFS_THRESHOLD="1000" setup_common_config "blob_files"
  $ cd $TESTTMP

Setup hg repo, create a commit there. No LFS blobs yet.
  $ testtool_drawdag --print-hg-hashes -R repo --derive-all --no-default-files <<EOF
  > A
  > # modify: A "s" "smallfile\n"
  > # message: A "add small file"
  > # bookmark: A master_bookmark
  > EOF
  A=dc58bd72a96c4e29ea83860a2113f63e3b7a18cf

Start Mononoke with LFS enabled.
  $ start_and_wait_for_mononoke_server
Start Mononoke API server, to serve LFS blobs
  $ lfs_uri="$(lfs_server --scuba-log-file "$TESTTMP/scuba.json")/repo"

Create a new client repository. Enable LFS there.
  $ hg clone -q mono:repo repo-lfs --noupdate
  $ cd repo-lfs
  $ setup_hg_modern_lfs "$lfs_uri" 1000B "$TESTTMP/lfs-cache1"

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

Update in the client repo
  $ hg pull -q
  $ hg update -r master_bookmark -q

Perform LFS push
  $ LONG="$(yes A 2>/dev/null | head -c 2000)"
  $ echo "$LONG" > lfs-largefile
  $ echo "${LONG}for-rename" > lfs-largefile-for-rename
  $ printf "$LONG\0\n" > lfs-binaryfile
  $ hg commit -Aqm "add lfs-large files"
  $ hg debugfilerevision
  *: add lfs-large files (glob)
   lfs-binaryfile: bin=1 lnk=0 flag=0 size=2001 copied=''
   lfs-largefile: bin=0 lnk=0 flag=0 size=2000 copied=''
   lfs-largefile-for-rename: bin=0 lnk=0 flag=0 size=2010 copied=''
  $ hg push -r . --to master_bookmark -v
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       246 (changelog)
       283  lfs-binaryfile
       282  lfs-largefile
       293  lfs-largefile-for-rename
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

# Check LFS logs
  $ wait_for_json_record_count "$TESTTMP/scuba.json" 4
  $ jq .int.client_attempt < "$TESTTMP/scuba.json"
  1
  1
  1
  1

# Rename a file
  $ hg mv lfs-largefile-for-rename lfs-largefile-renamed
  $ hg commit -Aqm "rename"
  $ hg push -r . --to master_bookmark -v
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       226 (changelog)
       379  lfs-largefile-renamed
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Verify that if we fail to upload LFS blobs first, the push fails
  $ cat >> .hg/hgrc << EOF
  > [lfs]
  > url=file://$TESTTMP/unused-dummystore
  > EOF

  $ echo "${LONG}ANOTHER-LFS" > f
  $ hg commit -m f -A f
  $ hg push -r . --to master_bookmark -v
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       176 (changelog)
       270  f
  remote: Command failed
  remote:   Error:
  remote:     While resolving Changegroup
  remote: 
  remote:     Caused by:
  remote:         0: While uploading File Blobs
  remote:         1: While decoding delta cache for file id ff714056cdbb88eef0578934980d740a05be8384, path f
  remote:         2: Blob is missing: alias.sha256.4200cad32a33c257258c559e80d19eedb89df109377863c6c16cf8416918b974
  abort: unexpected EOL, expected netstring digit
  [255]

  $ cd ..

Create a new client repository, using getpack (with its own cachepath)
  $ hg clone -q mono:repo repo-lfs3 --noupdate
  $ cd repo-lfs3
  $ setup_hg_modern_lfs "$lfs_uri" 1000B "$TESTTMP/lfs-cache3"

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > [remotefilelog]
  > fetchpacks = True
  > getpackversion = 2
  > cachepath=$TESTTMP/cachepath-alt
  > EOF

  $ hg pull -v
  pulling from mono:repo
 
  $ hg update -r master_bookmark -v
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ sha256sum lfs-largefile
  e2fff2ce58d585b4b0572e0a323f9e7e5f98cc641489e12c03c401d05d0e350d  lfs-largefile

  $ sha256sum lfs-largefile-renamed
  d19bca751e178f8cce59e1b872e0fd5857951c2577a2318aefad3253c317d982  lfs-largefile-renamed

  $ hg st --change . -C
  A lfs-largefile-renamed
    lfs-largefile-for-rename
  R lfs-largefile-for-rename

  $ hg debugfilerevision -r .^
  *: add lfs-large files (glob)
   lfs-binaryfile: bin=1 lnk=0 flag=0 size=2001 copied=''
   lfs-largefile: bin=0 lnk=0 flag=0 size=2000 copied=''
   lfs-largefile-for-rename: bin=0 lnk=0 flag=0 size=2010 copied=''

Make sure lfs-largefile isn't marked as is_binary by running blame:
  $ hg blame lfs-largefile | head -n 1
  *: A (glob)

Make sure lfs-binaryfile is marked as is_binary by running blame:
  $ hg blame lfs-binaryfile
  lfs-binaryfile: binary file

Now try with a small LFS cache size:
  $ hg clone -q mono:repo repo-lfs4 --noupdate
  $ cd repo-lfs4
  $ setup_hg_modern_lfs "$lfs_uri" 1000B "$TESTTMP/lfs-cache4"

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > cachepath=$TESTTMP/cachepath-alt2
  > [indexedlog]
  > lfs.max-bytes-per-log=1
  > lfs.max-log-count=1
  > EOF

  $ hg pull -v
  pulling from mono:repo
 
 Works even though the cache rotated out from under us.
  $ hg update -r master_bookmark -v
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
