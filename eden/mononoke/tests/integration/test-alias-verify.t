# Copyright (c) Meta Platforms, Inc. and affiliates.
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

  $ hginit_treemanifest repo
  $ cd repo
# Commit files
  $ echo f1 > f1
  $ hg commit -Aqm "f1"
  $ echo f2 > f2
  $ hg commit -Aqm "f2"
  $ echo f3 > f3
  $ hg commit -Aqm "f1"

  $ hg bookmark master_bookmark -r tip

  $ cd ..

  $ blobimport repo/.hg repo

  $ ls $TESTTMP/blobstore/blobs | grep "alias"
  blob-repo0000.alias.gitsha1.45d9e0e9fc8859787c33081dffdf12f41b54fcf3
  blob-repo0000.alias.gitsha1.8e1e71d5ce34c01b6fe83bc5051545f2918c8c2b
  blob-repo0000.alias.gitsha1.9de77c18733ab8009a956c25e28c85fe203a17d7
  blob-repo0000.alias.seeded_blake3.612c92c71a0f363c11b4bd01861e1a00a4bb663cd8473327ff36d77baef1bce9
  blob-repo0000.alias.seeded_blake3.c215015b756ebffc2d7a1c02926937f2a572e7143a021b7ed48aeec7d735d2b4
  blob-repo0000.alias.seeded_blake3.d454123e25942306cb55ff51fb153919dc4cb2f4d25d1bbdb4f40acfe60d8d67
  blob-repo0000.alias.sha1.1c49a440c352f3473efa9512255033b94dc7def0
  blob-repo0000.alias.sha1.aece6dfba588900e00d95601d22b4408d49580af
  blob-repo0000.alias.sha1.b4c4c2a335010e242576b05f3e0b673adfa58bc8
  blob-repo0000.alias.sha256.2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66
  blob-repo0000.alias.sha256.b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1
  blob-repo0000.alias.sha256.d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f

#Alias verfification with Sha256 alias
  $ aliasverify verify sha256 2>&1 | grep "Alias Verification"
  * Alias Verification continues: 0 errors found, repo: repo (glob)
  * Alias Verification finished: 0 errors found, repo: repo (glob)
#Alias verification with SeededBlake3 alias
  $ aliasverify verify seeded-blake3 2>&1 | grep "Alias Verification"
  * Alias Verification continues: 0 errors found, repo: repo (glob)
  * Alias Verification finished: 0 errors found, repo: repo (glob)
#Alias verification with Sha1 alias
  $ aliasverify verify sha1 2>&1 | grep "Alias Verification"
  * Alias Verification continues: 0 errors found, repo: repo (glob)
  * Alias Verification finished: 0 errors found, repo: repo (glob)
#Alias verification with GitSha1 alias
  $ aliasverify verify git-sha1 2>&1 | grep "Alias Verification"
  * Alias Verification continues: 0 errors found, repo: repo (glob)
  * Alias Verification finished: 0 errors found, repo: repo (glob)

  $ rm -rf $TESTTMP/blobstore/blobs/blob-repo0000.alias.*
  $ ls $TESTTMP/blobstore/blobs | grep "alias" | wc -l
  0

  $ aliasverify verify sha256 2>&1 | grep "Alias Verification"
  * Alias Verification continues: 3 errors found, repo: repo (glob)
  * Alias Verification finished: 3 errors found, repo: repo (glob)

#Missing Sha256 aliases
  $ aliasverify verify sha256 --debug 2>&1 | grep "Missing alias blob" | cut -d" " -f6- | sort
  Missing alias blob: alias Sha256(2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66), content_id ContentId(Blake2(1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a)), repo: repo
  Missing alias blob: alias Sha256(b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1), content_id ContentId(Blake2(1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada)), repo: repo
  Missing alias blob: alias Sha256(d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f), content_id ContentId(Blake2(7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd)), repo: repo
#Missing SeededBlake3 aliases
  $ aliasverify verify seeded-blake3 --debug 2>&1 | grep "Missing alias blob" | cut -d" " -f6- | sort
  Missing alias blob: alias Blake3(612c92c71a0f363c11b4bd01861e1a00a4bb663cd8473327ff36d77baef1bce9), content_id ContentId(Blake2(1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a)), repo: repo
  Missing alias blob: alias Blake3(c215015b756ebffc2d7a1c02926937f2a572e7143a021b7ed48aeec7d735d2b4), content_id ContentId(Blake2(7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd)), repo: repo
  Missing alias blob: alias Blake3(d454123e25942306cb55ff51fb153919dc4cb2f4d25d1bbdb4f40acfe60d8d67), content_id ContentId(Blake2(1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada)), repo: repo
#Missing Sha1 aliases
  $ aliasverify verify sha1 --debug 2>&1 | grep "Missing alias blob" | cut -d" " -f6- | sort
  Missing alias blob: alias Sha1(1c49a440c352f3473efa9512255033b94dc7def0), content_id ContentId(Blake2(1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada)), repo: repo
  Missing alias blob: alias Sha1(aece6dfba588900e00d95601d22b4408d49580af), content_id ContentId(Blake2(7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd)), repo: repo
  Missing alias blob: alias Sha1(b4c4c2a335010e242576b05f3e0b673adfa58bc8), content_id ContentId(Blake2(1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a)), repo: repo
#Missing GitSha1 aliases
  $ aliasverify verify git-sha1 --debug 2>&1 | grep "Missing alias blob" | cut -d" " -f6- | sort
  Missing alias blob: alias GitSha1(45d9e0e9fc8859787c33081dffdf12f41b54fcf3), content_id ContentId(Blake2(1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a)), repo: repo
  Missing alias blob: alias GitSha1(8e1e71d5ce34c01b6fe83bc5051545f2918c8c2b), content_id ContentId(Blake2(7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd)), repo: repo
  Missing alias blob: alias GitSha1(9de77c18733ab8009a956c25e28c85fe203a17d7), content_id ContentId(Blake2(1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada)), repo: repo

  $ ls $TESTTMP/blobstore/blobs | grep "alias" | wc -l
  0

#Generate Sha256 aliases
  $ aliasverify generate sha256 --debug 2>&1 | grep "Missing alias blob"  | cut -d" " -f6- | sort
  Missing alias blob: alias Sha256(2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66), content_id ContentId(Blake2(1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a)), repo: repo
  Missing alias blob: alias Sha256(b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1), content_id ContentId(Blake2(1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada)), repo: repo
  Missing alias blob: alias Sha256(d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f), content_id ContentId(Blake2(7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd)), repo: repo
#Generate SeededBlake3 aliases
  $ aliasverify generate seeded-blake3 --debug 2>&1 | grep "Missing alias blob"  | cut -d" " -f6- | sort
  Missing alias blob: alias Blake3(612c92c71a0f363c11b4bd01861e1a00a4bb663cd8473327ff36d77baef1bce9), content_id ContentId(Blake2(1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a)), repo: repo
  Missing alias blob: alias Blake3(c215015b756ebffc2d7a1c02926937f2a572e7143a021b7ed48aeec7d735d2b4), content_id ContentId(Blake2(7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd)), repo: repo
  Missing alias blob: alias Blake3(d454123e25942306cb55ff51fb153919dc4cb2f4d25d1bbdb4f40acfe60d8d67), content_id ContentId(Blake2(1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada)), repo: repo
#Generate Sha1 aliases
  $ aliasverify generate sha1 --debug 2>&1 | grep "Missing alias blob"  | cut -d" " -f6- | sort
  Missing alias blob: alias Sha1(1c49a440c352f3473efa9512255033b94dc7def0), content_id ContentId(Blake2(1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada)), repo: repo
  Missing alias blob: alias Sha1(aece6dfba588900e00d95601d22b4408d49580af), content_id ContentId(Blake2(7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd)), repo: repo
  Missing alias blob: alias Sha1(b4c4c2a335010e242576b05f3e0b673adfa58bc8), content_id ContentId(Blake2(1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a)), repo: repo
#Generate GitSha1 aliases
  $ aliasverify generate git-sha1 --debug 2>&1 | grep "Missing alias blob"  | cut -d" " -f6- | sort
  Missing alias blob: alias GitSha1(45d9e0e9fc8859787c33081dffdf12f41b54fcf3), content_id ContentId(Blake2(1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a)), repo: repo
  Missing alias blob: alias GitSha1(8e1e71d5ce34c01b6fe83bc5051545f2918c8c2b), content_id ContentId(Blake2(7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd)), repo: repo
  Missing alias blob: alias GitSha1(9de77c18733ab8009a956c25e28c85fe203a17d7), content_id ContentId(Blake2(1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada)), repo: repo

  $ ls $TESTTMP/blobstore/blobs | grep "alias"
  blob-repo0000.alias.gitsha1.45d9e0e9fc8859787c33081dffdf12f41b54fcf3
  blob-repo0000.alias.gitsha1.8e1e71d5ce34c01b6fe83bc5051545f2918c8c2b
  blob-repo0000.alias.gitsha1.9de77c18733ab8009a956c25e28c85fe203a17d7
  blob-repo0000.alias.seeded_blake3.612c92c71a0f363c11b4bd01861e1a00a4bb663cd8473327ff36d77baef1bce9
  blob-repo0000.alias.seeded_blake3.c215015b756ebffc2d7a1c02926937f2a572e7143a021b7ed48aeec7d735d2b4
  blob-repo0000.alias.seeded_blake3.d454123e25942306cb55ff51fb153919dc4cb2f4d25d1bbdb4f40acfe60d8d67
  blob-repo0000.alias.sha1.1c49a440c352f3473efa9512255033b94dc7def0
  blob-repo0000.alias.sha1.aece6dfba588900e00d95601d22b4408d49580af
  blob-repo0000.alias.sha1.b4c4c2a335010e242576b05f3e0b673adfa58bc8
  blob-repo0000.alias.sha256.2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66
  blob-repo0000.alias.sha256.b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1
  blob-repo0000.alias.sha256.d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f
