# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ cd $TESTTMP

Initialize test repo.
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

Create a nested directory structure.
  $ mkdir -p a{1,2}/b{1,2}/c{1,2}
  $ echo "1" | tee a{1,2}/{file,b{1,2}/{file,c{1,2}/file}} > /dev/null
  $ LC_ALL=C tree
  .
  |-- a1
  |   |-- b1
  |   |   |-- c1
  |   |   |   `-- file
  |   |   |-- c2
  |   |   |   `-- file
  |   |   `-- file
  |   |-- b2
  |   |   |-- c1
  |   |   |   `-- file
  |   |   |-- c2
  |   |   |   `-- file
  |   |   `-- file
  |   `-- file
  `-- a2
      |-- b1
      |   |-- c1
      |   |   `-- file
      |   |-- c2
      |   |   `-- file
      |   `-- file
      |-- b2
      |   |-- c1
      |   |   `-- file
      |   |-- c2
      |   |   `-- file
      |   `-- file
      `-- file
  
  14 directories, 14 files

Commit and note the root manifest hash.
  $ hg commit -Aqm "Create directory tree"
  $ BASE_MF_NODE=$(hg log -r . -T '{manifest}')

Modify only the files in directories ending in "1".
  $ echo "2" | tee a1/{file,b1/{file,c1/file}} > /dev/null
  $ hg status
  M a1/b1/c1/file
  M a1/b1/file
  M a1/file
  $ hg commit -Aqm "Modify all files named 'foo'"
  $ MF_NODE_1=$(hg log -r . -T '{manifest}')

Modify the files again.
  $ echo "3" | tee a1/{file,b1/{file,c1/file}} > /dev/null
  $ hg status
  M a1/b1/c1/file
  M a1/b1/file
  M a1/file
  $ hg commit -Aqm "Modify all files named 'foo' (again)"
  $ MF_NODE_2=$(hg log -r . -T '{manifest}')

Blobimport test repo.
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ setup_mononoke_config
  $ start_edenapi_server

Create and send complete tree request.
  $ edenapi_make_req tree > req.cbor <<EOF
  > {
  >   "rootdir": "",
  >   "mfnodes": ["$MF_NODE_1", "$MF_NODE_2"],
  >   "basemfnodes": ["$BASE_MF_NODE"],
  >   "depth": 2
  > }
  > EOF
  Reading from stdin
  Generated request: CompleteTreeRequest {
      rootdir: RepoPathBuf(
          "",
      ),
      mfnodes: [
          HgId("3d866afaa8cdb847e3800fef742c1fe9e741f75f"),
          HgId("8cad2f4cf4dc3d149356ed44a973fd3f6284deb6"),
      ],
      basemfnodes: [
          HgId("63e28e06687f0750555703a5993d72665ed21467"),
      ],
      depth: Some(
          2,
      ),
  }
  $ sslcurl -s "$EDENAPI_URI/repo/trees/complete" -d@req.cbor > res.cbor

Confirm that the response contains only directories whose files
were modified, and that each directory appears twice since the
files therein were modified in both commits.
  $ edenapi_read_res data ls res.cbor
  Reading from file: "res.cbor"
  3d866afaa8cdb847e3800fef742c1fe9e741f75f 
  acde3b6cc4d4d57e4cf533fd6f75ea3a4e4e49cb a1
  1f42a0d1c1a7e04fa016d2de7a5c72426d361dba a1/b1
  8cad2f4cf4dc3d149356ed44a973fd3f6284deb6 
  58b4b36319bf50b63fabfc64a8e02789fee97dac a1
  2eaa473729ad541dcccc4ebb74364ecce9b7e643 a1/b1
