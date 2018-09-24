  $ CACHEDIR=$PWD/cachepath
  $ . $TESTDIR/library.sh

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


# 1. Setup nolfs hg repo, create several commit to it
  $ hginit_treemanifest repo-hg-nolfs
  $ cd repo-hg-nolfs
  $ setup_hg_server

  $ LONG=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC

  $ echo $LONG > largefile
  $ hg commit -Aqm "add large file"

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
  $ cd ..

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs=
  > [lfs]
  > threshold=1000B
  > usercache=$TESTTMP/lfs-cache
  > EOF

# 3. Setup Mononoke API server.
  $ apiserver -p 0
  $ wait_for_apiserver

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

# 5. Hg push from hg client repo.
# small file push
  $ hg update master_bookmark
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg push -r . --to master_bookmark

  pushing rev ff7be8fc22d3 to destination ssh://user@dummy/repo-hg-nolfs bookmark master_bookmark
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  updating bookmark master_bookmark

# large file push
  $ echo $LONG > lfs-largefile
  $ SHA_LARGE_FILE=$(echo lfs-largefile | sha256sum)
  $ hg commit -Aqm "add lfs-large file"
  $ hg push -r . --to master_bookmark

  pushing rev 30f5daf5a5e2 to destination ssh://user@dummy/repo-hg-nolfs bookmark master_bookmark
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  lfs: uploading $SHA_LARGE_FILE (1.48 KB)
  lfs: processed: $SHA_LARGE_FILE
  remote: added 1 changesets with 1 changes to 1 files
  updating bookmark master_bookmark

# 5.a Check that Mononoke API server received POST repo/object/batch
  $ tail -n 2 $TESTTMP/apiserver.out | grep "200 POST /repo/objects/batch" | wc -l
  1

# 5.b Check that Mononoke API server received PUT repo/lfs/upload/{SHA}
  $ tail -n 2 $TESTTMP/apiserver.out | grep "200 PUT /repo/lfs/upload/$SHA_LARGE_FILE" | wc -l
  1
