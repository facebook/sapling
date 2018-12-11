  $ CACHEDIR=$PWD/cachepath
  $ . $TESTDIR/library.sh

# setup config repo

  $ REPOTYPE="blob:files"
  $ setup_common_config $REPOTYPE
  $ cd $TESTTMP

  $ hginit_treemanifest repo-hg-nolfs
  $ cd repo-hg-nolfs
  $ setup_hg_server

# Commit files
  $ echo f1 > f1
  $ hg commit -Aqm "f1"
  $ echo f2 > f2
  $ hg commit -Aqm "f2"
  $ echo f3 > f3
  $ hg commit -Aqm "f1"

  $ hg bookmark master_bookmark -r tip

  $ cd ..

  $ blobimport repo-hg-nolfs/.hg repo

  $ ls $TESTTMP/repo/blobs | grep "alias"
  blob-repo0000.alias.sha256.2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66
  blob-repo0000.alias.sha256.b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1
  blob-repo0000.alias.sha256.d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f

  $ aliasverify verify
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Process Changesets with ids: [1, 4) (glob)
  * Commit processed 0 (glob)
  * Alias Verification continues: 0 errors found (glob)
  * Alias Verification finished: 0 errors found (glob)


  $ rm -rf $TESTTMP/repo/blobs/blob-repo0000.alias.*
  $ ls $TESTTMP/repo/blobs | grep "alias" | wc -l
  0

  $ aliasverify verify
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Process Changesets with ids: [1, 4) (glob)
  * Commit processed 0 (glob)
  * Alias Verification continues: 3 errors found (glob)
  * Alias Verification finished: 3 errors found (glob)

  $ aliasverify verify --debug
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Process Changesets with ids: [1, 4) (glob)
  * Commit processed 0 (glob)
  * Missing alias blob: alias Sha256(d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f), content_id ContentId(Blake2(73f63d223a944d13b617aaefd255742e870cde0107d19c52d02d45b0d5ed690d)) (glob)
  * Missing alias blob: alias Sha256(b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1), content_id ContentId(Blake2(37db98f1ac02014d83cc39d05cfeaf1fee9798d398e57adf34d03d3b8f79fd42)) (glob)
  * Missing alias blob: alias Sha256(2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66), content_id ContentId(Blake2(b607a8c12dcea1585f68283b4c02b7013a8637165c83b068201fd32127dadbb6)) (glob)
  * Alias Verification continues: 3 errors found (glob)
  * Alias Verification finished: 3 errors found (glob)

  $ ls $TESTTMP/repo/blobs | grep "alias" | wc -l
  0

  $ aliasverify generate --debug
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Process Changesets with ids: [1, 4) (glob)
  * Commit processed 0 (glob)
  * Missing alias blob: alias Sha256(d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f), content_id ContentId(Blake2(73f63d223a944d13b617aaefd255742e870cde0107d19c52d02d45b0d5ed690d)) (glob)
  * Missing alias blob: alias Sha256(b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1), content_id ContentId(Blake2(37db98f1ac02014d83cc39d05cfeaf1fee9798d398e57adf34d03d3b8f79fd42)) (glob)
  * Missing alias blob: alias Sha256(2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66), content_id ContentId(Blake2(b607a8c12dcea1585f68283b4c02b7013a8637165c83b068201fd32127dadbb6)) (glob)
  * Alias Verification continues: 3 errors found (glob)
  * Alias Verification finished: 3 errors found (glob)

  $ ls $TESTTMP/repo/blobs | grep "alias"
  blob-repo0000.alias.sha256.2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66
  blob-repo0000.alias.sha256.b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1
  blob-repo0000.alias.sha256.d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f
