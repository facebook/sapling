  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

# Setup a repository config, with LFS enabled
  $ LFS_THRESHOLD=1000 setup_common_config
  $ cd $TESTTMP

# Setup nolfs hg repo, create several commit to it
  $ hginit_treemanifest repo-hg-nolfs
  $ cd repo-hg-nolfs
  $ setup_hg_server

# Commit a small file
  $ echo s > smallfile
  $ hg commit -Aqm "add small file"

  $ hg bookmark master_bookmark -r tip

# Blobimport hg nolfs to mononoke
  $ cd ..
  $ blobimport repo-hg-nolfs/.hg repo

# Setup hgrc to allow cloning the LFS repository
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

# Start the Mononoke LFS Server
  $ LFS_LOG="${TESTTMP}/lfs.log"
  $ lfs_uri="$(lfs_server --log "$LFS_LOG")/repo"

# Create a new hg repository clone, with a low threshold for new LFS files
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
  > usercache=$TESTTMP/lfs-cache1
  > url=$lfs_uri
  > EOF

  $ hg update master_bookmark -q

# Push a large file
  $ LONG="$(yes A 2>/dev/null | head -c 2000)"
  $ echo $LONG > lfs-largefile
  $ SHA_LARGE_FILE=$(sha256sum lfs-largefile | awk '{print $1;}')
  $ hg commit -Aqm "add lfs-large file"
  $ hg push -r . --to master_bookmark -v
  pushing rev d6c13aab6acd to destination ssh://user@dummy/repo-hg-nolfs bookmark master_bookmark
  searching for changes
  lfs: uploading 2a49733d725b4e6dfa94410d29da9e64803ff946339c54ecc471eccc951047fe (1.95 KB)
  lfs: processed: 2a49733d725b4e6dfa94410d29da9e64803ff946339c54ecc471eccc951047fe
  1 changesets found
  uncompressed size of bundle content:
       205 (changelog)
       282  lfs-largefile
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  updating bookmark master_bookmark

# Check that an upload was sent to the LFS server
  $ cat "$LFS_LOG"
  POST /repo/objects/batch 200 OK
  PUT /repo/upload/2a49733d725b4e6dfa94410d29da9e64803ff946339c54ecc471eccc951047fe/2000 200 OK
  POST /repo/objects/batch 200 OK
  $ truncate -s 0 "$LFS_LOG"

# Create a new hg repository, and update to the new file
  $ cd ..
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
  > url=$lfs_uri
  > [remotefilelog]
  > getpackversion = 2
  > EOF

  $ hg update master_bookmark -v
  resolving manifests
  lfs: downloading 2a49733d725b4e6dfa94410d29da9e64803ff946339c54ecc471eccc951047fe (1.95 KB)
  lfs: processed: 2a49733d725b4e6dfa94410d29da9e64803ff946339c54ecc471eccc951047fe
  getting lfs-largefile
  getting smallfile
  calling hook update.prefetch: edenscm.hgext.remotefilelog.wcpprefetch
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sha256sum lfs-largefile
  2a49733d725b4e6dfa94410d29da9e64803ff946339c54ecc471eccc951047fe  lfs-largefile

# Check that the LFS server received a download request (note: this is by Content ID)
  $ cat "$LFS_LOG"
  POST /repo/objects/batch 200 OK
  GET /repo/download/1267b7f944920cc2c6a5d48bcf0996735d3fe984b09d5d3bdbccb710c0b99635 200 OK

# Check that downloading file by its sha256 works
  $ DOWNLOAD_URL="${lfs_uri}/download_sha256/2a49733d725b4e6dfa94410d29da9e64803ff946339c54ecc471eccc951047fe"
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL"
  200
  $ sslcurl -s "$DOWNLOAD_URL" | sha256sum
  2a49733d725b4e6dfa94410d29da9e64803ff946339c54ecc471eccc951047fe  -
