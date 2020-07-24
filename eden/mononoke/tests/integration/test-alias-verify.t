# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

# setup config repo

  $ REPOTYPE="blob_files"
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

  $ ls $TESTTMP/blobstore/blobs | grep "alias"
  blob-repo0000.alias.gitsha1.45d9e0e9fc8859787c33081dffdf12f41b54fcf3
  blob-repo0000.alias.gitsha1.8e1e71d5ce34c01b6fe83bc5051545f2918c8c2b
  blob-repo0000.alias.gitsha1.9de77c18733ab8009a956c25e28c85fe203a17d7
  blob-repo0000.alias.sha1.1c49a440c352f3473efa9512255033b94dc7def0
  blob-repo0000.alias.sha1.aece6dfba588900e00d95601d22b4408d49580af
  blob-repo0000.alias.sha1.b4c4c2a335010e242576b05f3e0b673adfa58bc8
  blob-repo0000.alias.sha256.2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66
  blob-repo0000.alias.sha256.b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1
  blob-repo0000.alias.sha256.d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f

  $ aliasverify verify 2>&1 | grep "Alias Verification"
  * Alias Verification continues: 0 errors found (glob)
  * Alias Verification finished: 0 errors found (glob)


  $ rm -rf $TESTTMP/blobstore/blobs/blob-repo0000.alias.*
  $ ls $TESTTMP/blobstore/blobs | grep "alias" | count_stdin_lines
  0

  $ aliasverify verify 2>&1 | grep "Alias Verification"
  * Alias Verification continues: 3 errors found (glob)
  * Alias Verification finished: 3 errors found (glob)

  $ aliasverify verify --debug 2>&1 | grep "Missing alias blob"
  * Missing alias blob: alias Sha256(d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f), content_id ContentId(Blake2(7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd)) (glob)
  * Missing alias blob: alias Sha256(b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1), content_id ContentId(Blake2(1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada)) (glob)
  * Missing alias blob: alias Sha256(2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66), content_id ContentId(Blake2(1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a)) (glob)

  $ ls $TESTTMP/blobstore/blobs | grep "alias" | count_stdin_lines
  0

  $ aliasverify generate --debug 2>&1 | grep "Missing alias blob"
  * Missing alias blob: alias Sha256(d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f), content_id ContentId(Blake2(7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd)) (glob)
  * Missing alias blob: alias Sha256(b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1), content_id ContentId(Blake2(1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada)) (glob)
  * Missing alias blob: alias Sha256(2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66), content_id ContentId(Blake2(1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a)) (glob)

  $ ls $TESTTMP/blobstore/blobs | grep "alias"
  blob-repo0000.alias.sha256.2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66
  blob-repo0000.alias.sha256.b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1
  blob-repo0000.alias.sha256.d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f
