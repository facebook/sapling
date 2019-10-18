  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setup_common_config "blob:files"
  $ cd $TESTTMP

Setup repo and blobimport it

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma
  $ hg bookmark master_bookmark -r 'tip'
  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo

Start mononoke and the LFS Server

  $ mononoke
  $ wait_for_mononoke "$TESTTMP/repo"
  $ lfs_uri="$(lfs_server)/repo"

Setup client repo

  $ hgclone_treemanifest ssh://user@dummy/repo-hg hg-client
  $ cd hg-client
  $ setup_hg_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"

Create new commits

  $ mkdir b_dir
  $ hg up master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)
  $ echo "regular file" > small
  $ yes A 2>/dev/null | head -c 200 > large
  $ hg commit -Aqm "add files"
  $ hgmn push --debug
  pushing to ssh://user@dummy/repo
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  sending clienttelemetry command
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  lfs: computing set of blobs to upload
  lfs: need to transfer 2 objects (213 bytes)
  lfs: uploading f9f7889fcedc8580403673810e2be90e35980f10234f80d08a6497bbda16a245 (200 bytes)
  lfs: processed: f9f7889fcedc8580403673810e2be90e35980f10234f80d08a6497bbda16a245
  lfs: uploading 177507a4ee8737f0930661b3261e9e50edcec96d5cca59b7a4ef3b260936ce09 (13 bytes)
  lfs: processed: 177507a4ee8737f0930661b3261e9e50edcec96d5cca59b7a4ef3b260936ce09
  lfs: computing set of blobs to upload
  1 changesets found
  list of changesets:
  48d4d2fa17e54179e24de7fcb4a8ced38738ca4e
  sending unbundle command
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 232 bytes payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-part: "reply:pushkey" (params: 2 mandatory) supported
  bundle2-input-bundle: 1 parts total
  updating bookmark master_bookmark
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes

Clone the repository, and pull

  $ hgclone_treemanifest ssh://user@dummy/repo-hg hg-client
  $ cd hg-client
  $ setup_hg_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"
  $ hgmn pull
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  new changesets 48d4d2fa17e5
  $ hgmn up master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)
  $ sha256sum large
  f9f7889fcedc8580403673810e2be90e35980f10234f80d08a6497bbda16a245  large
