  $ . "${TEST_FIXTURES}/library.sh"

Setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config blob_files
  $ cd "$TESTTMP"

Setup repo
  $ hginit_treemanifest "${TESTTMP}/repo-hg"
  $ cd repo-hg
  $ touch a && hg addremove && hg ci -q -ma
  adding a
  $ hg log -T '{short(node)}\n'
  3903775176ed
  $ hg bookmark master_bookmark -r tip

  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo

Start Mononoke
  $ mononoke
  $ wait_for_mononoke
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
  > branchpattern=re:scratch/.+
  > EOF

setup repo-push and repo-pull
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ cd "${TESTTMP}/repo-push"
  $ setup_hg_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"

  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate
  $ cd "${TESTTMP}/repo-pull"
  $ setup_hg_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"

Do infinitepush (aka commit cloud) push
  $ cd "${TESTTMP}/repo-push"
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo new > newfile
  $ yes A 2>/dev/null | head -c 200 > large
  $ hg addremove -q
  $ hg ci -m new
  $ hgmn push ssh://user@dummy/repo -r . --bundle-store --debug --allow-anon
  pushing to ssh://user@dummy/repo
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  lfs: computing set of blobs to upload
  lfs: uploading f9f7889fcedc8580403673810e2be90e35980f10234f80d08a6497bbda16a245 (200 bytes)
  lfs: processed: f9f7889fcedc8580403673810e2be90e35980f10234f80d08a6497bbda16a245
  lfs: computing set of blobs to upload
  1 changesets found
  list of changesets:
  68394cf51f7e96952fe832a3c05d17a9b49e8b4b
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 3 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 1 advisory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-bundle: 0 parts total
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes

Try to pull it
  $ cd "${TESTTMP}/repo-pull"
  $ hgmn pull -r 68394cf51f7e96952fe832a3c05d17a9b49e8b4b
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 68394cf51f7e
