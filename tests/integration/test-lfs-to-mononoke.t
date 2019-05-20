  $ CACHEDIR=$PWD/cachepath
  $ . $TESTDIR/library.sh

# setup config repo

  $ REPOTYPE="blob:files"
  $ export LFS_THRESHOLD="1000"
  $ setup_common_config $REPOTYPE
  $ cd $TESTTMP

# Test Scenario (see more in test-lfs.t):
# 1. Setup server hg nolfs repo and make several commits to it
# 2. Blobimport hg nolfs to mononoke (blob:files).
#   2.a Motivation: Blobimport for now does not support import of lfs hg repos. (Error with RevlogRepo parsing).
#       So we need to blobimport hg repo without lsf extention.
#   2.b Motivation: For blob:files storage, is because we need to run Mononoke and Mononoke API server.
#       We cannot have 2 processes for 1 RocksDB repo, as RocksDb does not allows us to do that.
#   2.c Still Mononoke config is blobimported to Rocks DB. As Api server and Mononoke server are using them separately.
# 3. Setup Mononoke. Introduce LFS_THRESHOLD into Mononoke server config.
# 4. Setup Mononoke API server.
# 5. Clone hg nolfs repo to lfs client hg repo. Setup small threshold for large file size.
# 6. Hg push from hg client repo.
# 6.1 Hg push renamed file.
# 7. Hg pull from hg client repo.
#   7.1 Note: That lfs-cache folders should be different for both client repos

# 1. Setup nolfs hg repo, create several commit to it
  $ hginit_treemanifest repo-hg-nolfs
  $ cd repo-hg-nolfs
  $ setup_hg_server

# Commit small file
  $ echo s > smallfile
  $ hg commit -Aqm "add small file"

  $ hg bookmark master_bookmark -r tip

  $ cd ..

# 2. Blobimport hg nolfs to mononoke (blob:files).
#   2.a Motivation: Blobimport for now does not support import of lfs hg repos. (Error with RevlogRepo parsing).
#       So we need to blobimport hg repo without lsf extention.
#   2.b Motivation: For blob:files storage, is because we need to run Mononoke and Mononoke API server.
#       We cannot have 2 processes for 1 RocksDB repo, as RocksDb does not allows us to do that.
#   2.c Still Mononoke config is blobimported to Rocks DB. As Api server and Mononoke server are using them separately.
  $ blobimport repo-hg-nolfs/.hg repo

# 3. Setup Mononoke. Introduce LFS_THRESHOLD into Mononoke server config.
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

# 4. Setup Mononoke API server.
  $ APISERVER_PORT=$(get_free_socket)
  $ no_ssl_apiserver --http-host "127.0.0.1" --http-port $APISERVER_PORT
  $ wait_for_apiserver --no-ssl

# 5. Clone hg nolfs repo to lfs client hg repo. Setup small threshold for large file size.
  $ hgclone_treemanifest ssh://user@dummy/repo-hg-nolfs repo-hg-lfs --noupdate --config extensions.remotenames=
  $ cd repo-hg-lfs
  $ setup_hg_client
  $ setup_hg_lfs $APISERVER/repo 1000B $TESTTMP/lfs-cache1

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

# get smallfile
  $ hgmn pull -q
  devel-warn: applied empty changegroup at* (glob)
  $ hgmn update -r master_bookmark -q

# 6. Hg push from hg client repo.
  $ LONG=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC
  $ echo $LONG > lfs-largefile
  $ echo $LONG"for-rename" > lfs-largefile-for-rename
  $ hg commit -Aqm "add lfs-large files"
  $ hgmn push -r . --to master_bookmark -v
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev cbf96639d87c to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  lfs: need to transfer 2 objects (2.94 KB)
  lfs: uploading f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b (1.47 KB)
  lfs: processed: f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b
  lfs: uploading 8e861bc81e64491883d375bf97e9b5dbe4626f8651483cfa9c95db0e32da4a00 (1.48 KB)
  lfs: processed: 8e861bc81e64491883d375bf97e9b5dbe4626f8651483cfa9c95db0e32da4a00
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       231 (changelog)
       282  lfs-largefile
       293  lfs-largefile-for-rename
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

# 6.1 Rename file
  $ hg mv lfs-largefile-for-rename lfs-largefile-renamed
  $ hg commit -Aqm "rename"
  $ hgmn push -r . --to master_bookmark -v
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 5ff46b53dca4 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  lfs: uploading 8e861bc81e64491883d375bf97e9b5dbe4626f8651483cfa9c95db0e32da4a00 (1.48 KB)
  lfs: processed: 8e861bc81e64491883d375bf97e9b5dbe4626f8651483cfa9c95db0e32da4a00
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       226 (changelog)
       379  lfs-largefile-renamed
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

# Fail to push if LFS blob is not uploaded to the server
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > lfs=
  > [lfs]
  > url=file://$TESTTMP/unused-dummystore
  > EOF

  $ echo $LONG"ANOTHER-LFS" > f
  $ hg commit -m f -A f
  $ hgmn push -r . --to master_bookmark -v
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       176 (changelog)
       270  f
  remote: Command failed
  remote:   Error:
  remote:     While resolving Changegroup
  remote:   Root cause:
  remote:     MissingTypedKeyEntry(
  remote:         "alias.sha256.098e78d6738b5d3c2e01095bc16456f31e9f669e2eda7c6e11653fac755ce8a7"
  remote:     )
  remote:   Caused by:
  remote:     While uploading File Blobs
  remote:   Caused by:
  remote:     While decoding delta cache for file id c9d07fd7e2ec8a7a84ffa605085c8d98012cae47, path f
  remote:   Caused by:
  remote:     Missing typed key entry for key: alias.sha256.098e78d6738b5d3c2e01095bc16456f31e9f669e2eda7c6e11653fac755ce8a7
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ cd ..
7. Hg pull from hg client repo.
  $ hgclone_treemanifest ssh://user@dummy/repo-hg-nolfs repo-hg-lfs2 --noupdate --config extensions.remotenames=
  $ cd repo-hg-lfs2
  $ setup_hg_client
  $ setup_hg_lfs $APISERVER/repo 1000B $TESTTMP/lfs-cache2

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

  $ hgmn pull -v
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  all local heads known remotely
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  new changesets cbf96639d87c:5ff46b53dca4

  $ hgmn update -r master_bookmark -v
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  resolving manifests
  lfs: need to transfer 2 objects (2.94 KB)
  lfs: downloading 8e861bc81e64491883d375bf97e9b5dbe4626f8651483cfa9c95db0e32da4a00 (1.48 KB)
  lfs: processed: 8e861bc81e64491883d375bf97e9b5dbe4626f8651483cfa9c95db0e32da4a00
  lfs: downloading f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b (1.47 KB)
  lfs: processed: f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b
  getting lfs-largefile
  getting lfs-largefile-renamed
  getting smallfile
  calling hook update.prefetch: edenscm.hgext.remotefilelog.wcpprefetch
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat lfs-largefile
  AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC

  $ cat lfs-largefile-renamed
  AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCfor-rename

  $ hgmn st --change . -C
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  A lfs-largefile-renamed
    lfs-largefile-for-rename
  R lfs-largefile-for-rename

# 8.1 Change "sha256:oid" to an another valid oid to check sha1 consisnency
# Change "sha256:oid" to "sha256:f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b"
  $ echo $LONG"inconsistent" > inconsistent_file
  $ hg commit -Aqm "hash check"

  $ FILENODE_TO_CORRUPT=".hg/store/data/181f21543edce858fdf3e7ddba53facb9ba2e2dd/6179aa960f78800a0b879d461ea56c5bb17f468c"
  $ chmod 666 $FILENODE_TO_CORRUPT
  $ sed -i s/sha256:b3b32a0272a17de060bd061eba7617bcd0816da95b2d9d796535cf626bc26ef9/sha256:f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b/ $FILENODE_TO_CORRUPT

  $ hgmn push -r . --to master_bookmark -v
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev df4af074ec72 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  lfs: uploading f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b (1.48 KB)
  lfs: processed: f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       201 (changelog)
       286  inconsistent_file
  remote: Command failed
  remote:   Error:
  remote:     Error while uploading data for changesets, hashes: [HgChangesetId(HgNodeHash(Sha1(df4af074ec72d3695dfa50278202119bb9766fcf)))]
  remote:   Root cause:
  remote:     SharedError {
  remote:         error: Compat {
  remote:             error: SharedError { error: Compat { error: InconsistentEntryHash(FilePath(MPath("inconsistent_file")), HgNodeHash(Sha1(6179aa960f78800a0b879d461ea56c5bb17f468c)), HgNodeHash(Sha1(1c509d1a5c8ac7f7b8ac25dc417fca3acb882258))) } }
  remote:             
  remote:             While walking dependencies of Root Manifest with id HgManifestId(HgNodeHash(Sha1(1c03a06531d93ba681c1a01604921b1cb40361af)))
  remote:             
  remote:             While uploading child entries
  remote:             
  remote:             While processing entries
  remote:             
  remote:             While creating Changeset Some(HgNodeHash(Sha1(df4af074ec72d3695dfa50278202119bb9766fcf))), uuid: * (glob)
  remote:         }
  remote:     }
  remote:   Caused by:
  remote:     While creating Changeset Some(HgNodeHash(Sha1(df4af074ec72d3695dfa50278202119bb9766fcf))), uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
