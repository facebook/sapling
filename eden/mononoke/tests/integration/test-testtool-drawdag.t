# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setup_common_config "blob_files"

Create commits using an ASCII-art DAG

The DAG can be horizontal (using - to connect, going left to right), or
vertical (using | to connect, going bottom to top).

By default, each commit will add a single file named and containing the
same as the node name in the graph.  You can disable this with
--no-default-files.

You can also customize details about commits with special comments.

The tool will print the commit hash of each commit.

  $ testtool_drawdag -R repo --derive-all <<'EOF'
  >             I-J-K   P-Q-R     U-V-W
  >            /     \       \   /
  > A-B-C-D---H---L-M-N--O----S-T-X-Y-Z
  >    \     /          /
  >     E-F-G----------/
  > # modify: A path/to/file "add additional content"
  > # modify: A a_file old_content
  > # delete: Z path/to/file
  > # forget: Z Z
  > # copy: B b_file new_content A a_file
  > # bookmark: M main
  > # bookmark: Z zzzz
  > EOF
  A=668a9d94b2464ac0676742736f60b642609faf782f9efd6346becd2d2d9d4ba4
  B=235661fd7779a9838f8e084ed7194971650f9f116ee288a17f2a6919a14841ef
  C=5e86cfa880a3a086026850155086c0e3f7f91e97ab0a68a497dbebe0d3cdb731
  D=e6f94ffa3acff367e3cba25695c4deb98f377bf0e7714193a8c277e20758a517
  E=f275dc5e7633518b7f5b8c63989d17aabde567cdecd7f1c9adecd1334747cedb
  F=2ef7eef62a5df6c4232311bc26bab0e2fbe315dccfbfc8e52d17b7fd4b4bbee0
  G=04a632b4072700e08a097f4f3af0676695d9c1aee667d099f0d926e17ca58074
  H=fec0b37de79787148e26a6fde747cefd78eced22c3bc529327361da3e80e0059
  I=fc4430720884919018ab9c67394726872768f7d1950151dc8e4fc9cc72a34b72
  J=172090cfe7b53bb27afa02f40a812f94ed2d1676afc2352dd51c3dae9981a52f
  K=eaba89161959b630eb442c0be9e9a5bcbb56a807e0fbc52003a6762e2a421e34
  L=0633a0f60b0a9980f7c1e6ca75beeebc4d7d1463426f17383293b13427aa7397
  M=29a42eb1636442a84838382a87bd50249d5c5201f363f6340f97ea50ce500f0e
  N=d0172157d156bd460263ef3d0bb95ab2c8f101ea3bf7b42999f401df044c5852
  O=195008295ba2b581870e00081606c221fe7d685dd887272eac70696747e6e253
  P=93fb02ab5bc2a5ce4fb47586a818e589bff242d4f38acf171132a936e3b483b7
  Q=457604f9b00fe70d661363457439230ed7388ac75c8888dc71b95d5374d87553
  R=fded11d5c4c4f8bd7285e7c7a8c84feb8a6a46b264cd44c0ef4a7121d45bc813
  S=92c8f69f32494ed97a6d58a884e53659ba700bef0b687a46de94d34ca44dbc1a
  T=30649a9045a5a47eb3d61a761ba90866cd9606ccfaceabb3eebaf74d9ee4a432
  U=cad3b53d7ea1f61105f42af83bf2901b01ffead46a47b9e102ef8905ebe8973f
  V=28c133feff87cd8ad602e8ceb8a27bab515708674453f725d17057048a8ef67b
  W=0d50131ffe46b4052ed85d6bc352b93e7e4ef9d5a69b3aeb6af9016f257a7a71
  X=b8eecc665a3fa0a2db4269503b8662b588cdaa0a951a5280fbefa8e1b3adb1ca
  Y=0496bee16414ecd92d98379f536e2306252990c72001f64fe01a6229a3a2c136
  Z=996698aaf45736eedc0b2496616255ab81f822b831a01723df7a018fc98ea3ad

  $ mononoke_newadmin fetch -R repo -i $Z -p Z
  Error: Path does not exist: Z
  [1]
  $ mononoke_newadmin fetch -R repo -i $Z -p Y
  File-Type: regular
  Size: 1
  Content-Id: 35ccfd1831564764439349755068b3400612b615d0c85e4d73af0cee786c963e
  Sha1: 23eb4d3f4155395a74e9d534f97ff4c1908f5aac
  Sha256: 18f5384d58bcb1bba0bcd9e6a6781d1a6ac2cc280c330ecbab6cb7931b721552
  Git-Sha1: 24de910c13bb1e60fc5ec37a1058d356b1f2fa4d
  
  Y

The graph can be extended with more commits.  The node names don't
need to match the previous graph (although it's probably a good idea).

  $ testtool_drawdag -R repo <<'EOF'
  >        XX    # modify: XX path/to/file "more additional content"
  >       /  \   # bookmark: XX xxxx
  >     D1    W2
  >     |     |
  >     |     W1
  >     D0    |  # exists: D0 e6f94ffa3acff367e3cba25695c4deb98f377bf0e7714193a8c277e20758a517
  >           W0 # exists: W0 0d50131ffe46b4052ed85d6bc352b93e7e4ef9d5a69b3aeb6af9016f257a7a71
  > EOF
  D0=e6f94ffa3acff367e3cba25695c4deb98f377bf0e7714193a8c277e20758a517
  D1=5e0970833eab2cd88e339ff162619968345677077b243a1b0f415f7f266c75b1
  W0=0d50131ffe46b4052ed85d6bc352b93e7e4ef9d5a69b3aeb6af9016f257a7a71
  W1=48288d4775d3e44b2c7a97643638c6c6ce162efb8fef108080ac86a9001ff605
  W2=28354dd9e37c4b5aadd24f3e8eb4699f9a01b6e4b48554f3808fb432da0fafc4
  XX=f6509cec643ab922cf4dbd0583f8c8683701259daf535b4e80da448c1810d6d4

Test HG hashes and setting the commit message and author:
  $ testtool_drawdag -R repo --print-hg-hashes <<'EOF'
  > AA-BB-CC
  > # modify: AA file "content"
  > # message: CC "just a commit message"
  > # author: CC "Test Y. Testovich <test@meta.com>"
  > EOF
  AA=73a53b07af3d15928010e8d72630750e98875c4a
  BB=d005ae50b8698478630ac396568f337d3c24063c
  CC=2d21fd53ce56b2c798dab5af7f2fce72411fcb6e

  $ mononoke_newadmin fetch -R repo --hg-id $AA -p file
  File-Type: regular
  Size: 7
  Content-Id: 95b845f64a4cb04cf60a55e9715210fcea6e187813221ab49e766b1478dbaa13
  Sha1: 040f06fd774092478d450774f5ba30c5da78acc8
  Sha256: ed7002b439e9ac845f22357d822bac1444730fbdb6016d3ec9432297b9ec9f73
  Git-Sha1: 6b584e8ece562ebffc15d38808cd6b98fc3d97ea
  
  content

  $ mononoke_newadmin fetch -R repo --hg-id $CC
  BonsaiChangesetId: 3a29cc35f0e5cdfe1305159710f1baf48c08034a1964e871b24559d9ef5fcbee
  Author: Test Y. Testovich <test@meta.com>
  Message: just a commit message
  FileChanges:
  	 ADDED/MODIFIED: CC 151a580a9eacb832365f854fabde6941930ddf5baa1cef1bfb0e411bdde2df94
  
