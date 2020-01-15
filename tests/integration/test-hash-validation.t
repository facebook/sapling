
  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_files"
  $ cd $TESTTMP

setup repo

  $ hg init repo-hg

setup hg server repo
  $ cd repo-hg
  $ setup_hg_server
  $ cd $TESTTMP

setup client repo2
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-client --noupdate -q
  $ cd repo-client
  $ setup_hg_client

make a few commits on the server
  $ cd $TESTTMP/repo-hg
  $ drawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

Corrupt blobs by replacing one content blob with another
  $ cd blobstore/blobs
  $ cp blob-repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d blob-repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9

start mononoke

  $ mononoke
  $ wait_for_mononoke


Prefetch should fail with corruption error
  $ cd $TESTTMP/repo-client
  $ hgmn pull
  pulling from ssh://user@dummy/repo
  warning: stream clone requested but client is missing requirements: lz4revlog
  (see https://www.mercurial-scm.org/wiki/MissingRequirement for more information)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 0 changes to 0 files
  adding remote bookmark master_bookmark

  $ hgmn prefetch -r ":"
  remote: Command failed
  remote:   Error:
  remote:     Corrupt hg filenode returned: 005d992c5dcf32993668f7cede29d296c494a5d9 != a2e456504a5e61f763f1a0b36a6c247c7541b2b3
  remote:   Root cause:
  remote:     CorruptHgFileNode {
  remote:         expected: HgFileNodeId(
  remote:             HgNodeHash(
  remote:                 Sha1(005d992c5dcf32993668f7cede29d296c494a5d9),
  remote:             ),
  remote:         ),
  remote:         actual: HgFileNodeId(
  remote:             HgNodeHash(
  remote:                 Sha1(a2e456504a5e61f763f1a0b36a6c247c7541b2b3),
  remote:             ),
  remote:         ),
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 2)
  [255]
