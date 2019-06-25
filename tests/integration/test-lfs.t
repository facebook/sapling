  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

#setup config repo
  $ setup_common_config
  $ cd $TESTTMP

# Test Scenario:
# 1. Setup server hg nolfs repo and make several commits to it
#   1.a Note that it should have a master_bookmark for easy push to it from a different repo.
#   1.b Note that server and client repos are supposed to be [treemanifest]treeonly=True, to avoid pushes to hybrid state
# 2. Blobimport hg nolfs to mononoke (rocksDB).
#   2.a Motivation: Blobimport for now does not support import of lfs hg repos. (Error with RevlogRepo parsing).
#       So we need to blobimport hg repo without lsf extention.
# 3. Setup Mononoke API server.
# 4. Clone hg nolfs repo to lfs client hg repo. Setup small threshold for large file size.
# 5. Hg push from hg client repo.
#   5.a Check that Mononoke API server received POST repo/object/batch
#   5.b Check that Mononoke API server received PUT repo/lfs/upload/{SHA}
# 6. Create another Hg client repo. and pull to it.
#   7.a Check that Mononoke API server received POST repo/object/batch
#   7.b Check that Mononoke API server received GET repo/lfs/download/{SHA}


# 1. Setup nolfs hg repo, create several commit to it
  $ hginit_treemanifest repo-hg-nolfs
  $ cd repo-hg-nolfs
  $ setup_hg_server

# Commit small file
  $ echo s > smallfile
  $ hg commit -Aqm "add small file"

  $ hg bookmark master_bookmark -r tip

# 2. Blobimport hg nolfs to mononoke (rocksDB).
  $ cd ..
  $ blobimport repo-hg-nolfs/.hg repo

  $ cd repo-hg-nolfs
  $ cat >> $HGRCPATH << EOF
  > [treemanifest]
  > treeonly=True
  > EOF

  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > lfs=
  > [lfs]
  > threshold=1000B
  > usercache=$TESTTMP/lfs-cache
  > EOF

  $ cd ..

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs=
  > [lfs]
  > threshold=1000B
  > usercache=$TESTTMP/lfs-cache
  > EOF

# 3. Setup Mononoke API server.
  $ APISERVER_PORT=$(get_free_socket)
  $ no_ssl_apiserver --http-host "127.0.0.1" --http-port $APISERVER_PORT
  $ wait_for_apiserver --no-ssl

# 4. Clone hg nolfs repo to lfs client hg repo. Setup small threshold for large file size.
  $ hgclone_treemanifest ssh://user@dummy/repo-hg-nolfs repo-hg-lfs --noupdate --config extensions.remotenames=
  $ cd repo-hg-lfs
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > lfs=
  > [lfs]
  > threshold=1000B
  > usercache=$TESTTMP/lfs-cache
  > url=$APISERVER/repo
  > EOF

  $ hg update master_bookmark -q

# ================ large file PUSH ===================
  $ LONG=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC
  $ echo $LONG > lfs-largefile
  $ SHA_LARGE_FILE=$(sha256sum lfs-largefile | awk '{print $1;}')
  $ hg commit -Aqm "add lfs-large file"
  $ hg push -r . --to master_bookmark -v
  pushing rev 0db8825b9792 to destination ssh://user@dummy/repo-hg-nolfs bookmark master_bookmark
  searching for changes
  lfs: uploading f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b (1.47 KB)
  lfs: processed: f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b
  lfs: uploading f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b (1.47 KB)
  lfs: processed: f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b
  1 changesets found
  uncompressed size of bundle content:
       205 (changelog)
       282  lfs-largefile
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  updating bookmark master_bookmark

# 5.a Check that Mononoke API server received POST repo/object/batch
  $ tail -n 2 $TESTTMP/apiserver.out | grep "200 POST /repo/objects/batch" | wc -l
  1

# 5.b Check that Mononoke API server received PUT repo/lfs/upload/{SHA}
  $ tail -n 2 $TESTTMP/apiserver.out | grep "200 PUT /repo/lfs/upload/$SHA_LARGE_FILE" | wc -l
  1

  $ cd ..

# ===================== large file PULL ========================
# 6. Create another Hg client repo. and pull to it.
  $ hgclone_treemanifest ssh://user@dummy/repo-hg-nolfs repo-hg-lfs2 --noupdate --config extensions.remotenames=
  $ cd repo-hg-lfs2
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > lfs=
  > [lfs]
  > threshold=1000B
  > usercache=$TESTTMP/lfs-cache2
  > url=$APISERVER/repo
  > EOF

  $ hg update master_bookmark -v
  resolving manifests
  lfs: downloading f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b (1.47 KB)
  lfs: processed: f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b
  getting lfs-largefile
  getting smallfile
  calling hook update.prefetch: edenscm.hgext.remotefilelog.wcpprefetch
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

#   7.a Check that Mononoke API server received POST repo/object/batch
  $ tail -n 2 $TESTTMP/apiserver.out | grep "200 POST /repo/objects/batch" | wc -l
  1

#   7.b Check that Mononoke API server received GET repo/lfs/download/{SHA}
  $ tail -n 2 $TESTTMP/apiserver.out | grep "200 GET /repo/lfs/download/$SHA_LARGE_FILE" | wc -l
  1
