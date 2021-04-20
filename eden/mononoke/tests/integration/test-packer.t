# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

# setup config repo

  $ REPOTYPE="blob_files"
  $ MULTIPLEXED=1
  $ PACK_BLOB=1
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

# Get the space consumed by the blobs as-is
  $ du -k $TESTTMP/blobstore/0/blobs/
  120	$TESTTMP/blobstore/0/blobs/
# Pack all blobs in interesting groups
  $ packer --zstd-level 10 --inner-blobstore-id 0 <<EOF
  > repo0000.alias.gitsha1.45d9e0e9fc8859787c33081dffdf12f41b54fcf3
  > repo0000.alias.gitsha1.8e1e71d5ce34c01b6fe83bc5051545f2918c8c2b
  > repo0000.alias.gitsha1.9de77c18733ab8009a956c25e28c85fe203a17d7
  > repo0000.alias.sha1.1c49a440c352f3473efa9512255033b94dc7def0
  > repo0000.alias.sha1.aece6dfba588900e00d95601d22b4408d49580af
  > repo0000.alias.sha1.b4c4c2a335010e242576b05f3e0b673adfa58bc8
  > repo0000.alias.sha256.2ba85baaa7922ff4c0dfdbc00fd07bd69dcb1dce745c6a8c676fe8b5642a0d66
  > repo0000.alias.sha256.b9a294f298d0ed2b65ca4488a42b473ff5f75d0b9843cbea84e1b472f9a514d1
  > repo0000.alias.sha256.d690916cdea320e620748799a2051a0f4e07d6d0c3e2bc199ea3c69e0c0b5e4f
  > EOF
  $ packer --zstd-level 10 --inner-blobstore-id 0 << EOF
  > repo0000.changeset.blake2.4767a96a14ccc03532e1be513de309b79397428535997d23ed2b755f178e83aa
  > repo0000.changeset.blake2.6d2e07c7403cc23e3dc516c2f6f76eb228bd280d87d73f236e6e5faa23c07cde
  > repo0000.changeset.blake2.d4c50ea4de683be19d2cc3dd7d56e429e378394c33ec0785dba304565cd67303
  > repo0000.hgchangeset.sha1.01463087777a97fc272718439b76fa600d471922
  > repo0000.hgchangeset.sha1.3f25c66441ca32eec3db952b59f642b9a475714e
  > repo0000.hgchangeset.sha1.fdef3a947e6adb8771a7c4b07b1836de9805647e
  > EOF
  $ packer --zstd-level 10 --inner-blobstore-id 0 << EOF
  > repo0000.content.blake2.1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a
  > repo0000.content.blake2.1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada
  > repo0000.content.blake2.7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd
  > EOF
  $ packer --zstd-level 10 --inner-blobstore-id 0 << EOF
  > repo0000.content_metadata.blake2.1a3f1094cdae123ec6999b7baf4211ffd94f47970bedd71e13ec07f24a9aba6a
  > repo0000.content_metadata.blake2.1af04efffa454f843420a538617f0c4166550da421b65a59ed95a85b43a25ada
  > repo0000.content_metadata.blake2.7ee06cac57ab4267c097ebc8ec36e903fb3c25867934fe360e069ea1ab2ed7fd
  > EOF
  $ packer --zstd-level 10 --inner-blobstore-id 0 << EOF
  > repo0000.filenode_lookup.34ff446a1e93f08eb1952478f434be0f08acc11bba09ea27e8176a62b30351b5
  > repo0000.filenode_lookup.cef879bbceca92e235a8061b10a3ac2c2efd406c72ea66d3e92738865f6d5718
  > repo0000.filenode_lookup.edaf6a3edbea1dc89034552baa60a0f7466923381c86afe50b8ef1d2789943ec
  > repo0000.hgfilenode.sha1.4cd1f7cc2c0c4e2dc17255b533e40a2f76736d9f
  > repo0000.hgfilenode.sha1.5a2cc92092e0c6785f2f6df602a9e4e70d3d5a7e
  > repo0000.hgfilenode.sha1.a6d0b2a4b39d001ede9efadbd1063fa5cc20065a
  > EOF
  $ packer --zstd-level 10 --inner-blobstore-id 0 << EOF
  > repo0000.hgmanifest.sha1.060af2899bdf48f768664390071fa2284e1bb2bb
  > repo0000.hgmanifest.sha1.3ea4d49c88c9a6e19670e35d1039b979bc949336
  > repo0000.hgmanifest.sha1.b8cc715336f05c0a40ee4549c3f54ca3912cd605
  > EOF

# Get the space consumed by the packs
  $ du -k $TESTTMP/blobstore/0/blobs/
  24	$TESTTMP/blobstore/0/blobs/
