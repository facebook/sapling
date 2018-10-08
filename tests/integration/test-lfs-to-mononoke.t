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
# 7. Hg pull from hg client repo.


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
  $ blobimport files repo-hg-nolfs/.hg repo

# 3. Setup Mononoke. Introduce LFS_THRESHOLD into Mononoke server config.
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

# 4. Setup Mononoke API server.
  $ no_ssl_apiserver --http-host "127.0.0.1" --http-port $(get_free_socket)
  $ wait_for_apiserver --no-ssl

# 5. Clone hg nolfs repo to lfs client hg repo. Setup small threshold for large file size.
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

# get smallfile
  $ hgmn pull -q
  devel-warn: applied empty changegroup at* (glob)
  $ hgmn update -r master_bookmark -q

# 6. Hg push from hg client repo.
  $ LONG=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC
  $ echo $LONG > lfs-largefile
  $ hg commit -Aqm "add lfs-large file"
  $ hgmn push -r . --to master_bookmark -v
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 0db8825b9792 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  lfs: uploading f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b (1.47 KB)
  lfs: processed: f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       205 (changelog)
       282  lfs-largefile
  remote: * ERRO Command failed, remote: true, error: Error while uploading data for changesets, hashes: [HgNodeHash(Sha1(0db8825b9792942920e0694b48c74dc440c26fd3))], root_cause: SharedError { (glob)
  remote:     error: Compat {
  remote:         error: SharedError { error: Compat { error: InconsistentEntryHash(FilePath(MPath([108, 102, 115, 45, 108, 97, 114, 103, 101, 102, 105, 108, 101] "lfs-largefile")), HgNodeHash(Sha1(1c509d1a5c8ac7f7b8ac25dc417fca3acb882258)), HgNodeHash(Sha1(c8702beecf6ea0642781455b62d42ed5b66a5391))) } }
  remote:          (re)
  remote:         While walking dependencies of Root Manifest with id HgManifestId(HgNodeHash(Sha1(50950f048a4d82fd3f641344c69855e9d6987da7)))
  remote:          (re)
  remote:         While uploading child entries
  remote:          (re)
  remote:         While processing entries
  remote:          (re)
  remote:         While creating Changeset Some(HgNodeHash(Sha1(0db8825b9792942920e0694b48c74dc440c26fd3))), uuid: * (glob)
  remote:     }
  remote: }, backtrace: , cause: While creating Changeset Some(HgNodeHash(Sha1(0db8825b9792942920e0694b48c74dc440c26fd3))), uuid: *, session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

# 7. Hg pull from hg client repo.
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
  > usercache=$TESTTMP/lfs-cache
  > url=$APISERVER/repo
  > EOF

  $ hgmn pull -v
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  no changes found
  adding changesets
  devel-warn: applied empty changegroup at* (glob)
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  $ hgmn update -r master_bookmark -v
  resolving manifests
  getting smallfile
  calling hook update.prefetch: hgext.remotefilelog.wcpprefetch
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
