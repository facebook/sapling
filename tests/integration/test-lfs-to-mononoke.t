  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

Setup repo config (we use blob:files to share across Mononoke and API Server):
  $ LFS_THRESHOLD="1000" setup_common_config "blob:files"
  $ cd $TESTTMP

Setup hg repo, create a commit there. No LFS blobs yet.
  $ hginit_treemanifest repo-hg-nolfs
  $ cd repo-hg-nolfs
  $ setup_hg_server

Commit small file
  $ echo s > smallfile
  $ hg commit -Aqm "add small file"
  $ hg bookmark master_bookmark -r tip
  $ cd ..

Blobimport the hg repo to Mononoke
  $ blobimport repo-hg-nolfs/.hg repo

Start Mononoke with LFS enabled.
  $ mononoke
  $ wait_for_mononoke "$TESTTMP/repo"

Start Mononoke API server, to serve LFS blobs
  $ lfs_uri="$(lfs_server)/repo"

Create a new client repository. Enable LFS there.
  $ hgclone_treemanifest ssh://user@dummy/repo-hg-nolfs repo-hg-lfs --noupdate --config extensions.remotenames=
  $ cd repo-hg-lfs
  $ setup_hg_client
  $ setup_hg_lfs "$lfs_uri" 1000B "$TESTTMP/lfs-cache1"

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

Update in the client repo
  $ hgmn pull -q
  devel-warn: applied empty changegroup at* (glob)
  $ hgmn update -r master_bookmark -q

Perform LFS push
  $ LONG="$(yes A 2>/dev/null | head -c 2000)"
  $ echo "$LONG" > lfs-largefile
  $ echo "${LONG}for-rename" > lfs-largefile-for-rename
  $ hg commit -Aqm "add lfs-large files"
  $ hgmn push -r . --to master_bookmark -v
  pushing rev 99765c8d839c to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  lfs: need to transfer 2 objects (3.92 KB)
  lfs: uploading e2fff2ce58d585b4b0572e0a323f9e7e5f98cc641489e12c03c401d05d0e350d (1.95 KB)
  lfs: processed: e2fff2ce58d585b4b0572e0a323f9e7e5f98cc641489e12c03c401d05d0e350d
  lfs: uploading d19bca751e178f8cce59e1b872e0fd5857951c2577a2318aefad3253c317d982 (1.96 KB)
  lfs: processed: d19bca751e178f8cce59e1b872e0fd5857951c2577a2318aefad3253c317d982
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

# Rename a file
  $ hg mv lfs-largefile-for-rename lfs-largefile-renamed
  $ hg commit -Aqm "rename"
  $ hgmn push -r . --to master_bookmark -v
  pushing rev c651f052c52d to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
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

Verify that if we fail to upload LFS blobs first, the push fails
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > lfs=
  > [lfs]
  > url=file://$TESTTMP/unused-dummystore
  > EOF

  $ echo "${LONG}ANOTHER-LFS" > f
  $ hg commit -m f -A f
  $ hgmn push -r . --to master_bookmark -v
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
  remote:     ContentBlobByAliasMissing(
  remote:         Sha256(4200cad32a33c257258c559e80d19eedb89df109377863c6c16cf8416918b974),
  remote:     )
  remote:   Caused by:
  remote:     While uploading File Blobs
  remote:   Caused by:
  remote:     While decoding delta cache for file id ff714056cdbb88eef0578934980d740a05be8384, path f
  remote:   Caused by:
  remote:     Content blob missing for id: 4200cad32a33c257258c559e80d19eedb89df109377863c6c16cf8416918b974
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ cd ..

Create a new client repository, using getpack (with its own cachepath)
  $ hgclone_treemanifest ssh://user@dummy/repo-hg-nolfs repo-hg-lfs3 --noupdate --config extensions.remotenames=
  $ cd repo-hg-lfs3
  $ setup_hg_client
  $ setup_hg_lfs "$lfs_uri" 1000B "$TESTTMP/lfs-cache3"

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > [remotefilelog]
  > fetchpacks = True
  > getpackversion = 2
  > cachepath=$TESTTMP/cachepath-alt
  > EOF

  $ hgmn pull -v
  pulling from ssh://user@dummy/repo
  searching for changes
  all local heads known remotely
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  new changesets 99765c8d839c:c651f052c52d
 
  $ hgmn update -r master_bookmark -v
  resolving manifests
  lfs: need to transfer 2 objects (3.92 KB)
  lfs: downloading d19bca751e178f8cce59e1b872e0fd5857951c2577a2318aefad3253c317d982 (1.96 KB)
  lfs: processed: d19bca751e178f8cce59e1b872e0fd5857951c2577a2318aefad3253c317d982
  lfs: downloading e2fff2ce58d585b4b0572e0a323f9e7e5f98cc641489e12c03c401d05d0e350d (1.95 KB)
  lfs: processed: e2fff2ce58d585b4b0572e0a323f9e7e5f98cc641489e12c03c401d05d0e350d
  getting lfs-largefile
  getting lfs-largefile-renamed
  getting smallfile
  calling hook update.prefetch: edenscm.hgext.remotefilelog.wcpprefetch
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ sha256sum lfs-largefile
  e2fff2ce58d585b4b0572e0a323f9e7e5f98cc641489e12c03c401d05d0e350d  lfs-largefile

  $ sha256sum lfs-largefile-renamed
  d19bca751e178f8cce59e1b872e0fd5857951c2577a2318aefad3253c317d982  lfs-largefile-renamed

  $ hgmn st --change . -C
  A lfs-largefile-renamed
    lfs-largefile-for-rename
  R lfs-largefile-for-rename

Change "sha256:oid" to an another valid oid to check sha1 consisnency
  $ echo "${LONG}inconsistent" > inconsistent_file
  $ hg commit -Aqm "hash check"

  $ PACK_TO_CORRUPT=".hg/store/packs/53030272778e08be8e520a61c0848183520e58ba.datapack"
  $ chmod 666 "$PACK_TO_CORRUPT"
  $ sed -i s/sha256:f79cf994214182953d15cd20b2a92731052ddc9a02f4c60518dc78d7a005cca9/sha256:e2fff2ce58d585b4b0572e0a323f9e7e5f98cc641489e12c03c401d05d0e350d/ "$PACK_TO_CORRUPT"

  $ hgmn push -r . --to master_bookmark -v
  pushing rev 77f499cb0645 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       201 (changelog)
       286  inconsistent_file
  remote: Command failed
  remote:   Error:
  remote:     Error while uploading data for changesets, hashes: [HgChangesetId(HgNodeHash(Sha1(77f499cb064550703c65d943b8ce1b982a1293cd)))]
  remote:   Root cause:
  remote:     SharedError {
  remote:         error: Compat {
  remote:             error: SharedError { error: Compat { error: InconsistentEntryHash(FilePath(MPath("inconsistent_file")), HgNodeHash(Sha1(ef5953d600ca68bacb539eab8dffb415441213bb)), HgNodeHash(Sha1(232ec9b974a9df3d48c2b740396691fb8939976c))) } }
  remote:             
  remote:             While walking dependencies of Root Manifest with id HgManifestId(HgNodeHash(Sha1(a1da9053000e0fb9217762d82ba5db793cfb26ce)))
  remote:             
  remote:             While uploading child entries
  remote:             
  remote:             While processing entries
  remote:             
  remote:             While creating Changeset Some(HgNodeHash(Sha1(77f499cb064550703c65d943b8ce1b982a1293cd))), uuid: * (glob)
  remote:         },
  remote:     }
  remote:   Caused by:
  remote:     While creating Changeset Some(HgNodeHash(Sha1(77f499cb064550703c65d943b8ce1b982a1293cd))), uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

