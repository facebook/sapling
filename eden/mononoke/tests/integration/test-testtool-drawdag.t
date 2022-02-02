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

  $ mononoke_testtool drawdag -R repo <<'EOF'
  >             I-J-K   P-Q-R     U-V-W
  >            /     \       \   /
  > A-B-C-D---H---L-M-N--O----S-T-X-Y-Z
  >    \     /          /
  >     E-F-G----------/
  > # modify: A path/to/file "add additional content"
  > # delete: Z path/to/file
  > # bookmark: M main
  > # bookmark: Z zzzz
  > EOF
  *] Reloading redacted config from configerator (glob)
  A=1c68a7b9eb5c92651e6e08a2d15a811f539f4051a23bc16d8f3bec7b69cc2fb2
  B=f2133d7c1ed18900e4c2ce57aff0d32af25ba0d0de9128fa79056aea366cd46f
  C=0750cdede19930eeaaf9a5653156112e7b1ae382010a00936c6319bf92351ebb
  D=971c152b1c5d22763033d2fc23ac40c1e785a3c50df28e37b69bf188f2d949b5
  E=b2ee1306f4b80638b11ee9893952807e4298f9c686c32eef55c09ef9753e39ef
  F=4e33d245d2e3fe6456fd4e9cb0c14f87f2a77385b6008c47b88d136b3105685d
  G=08d06276b2c3b348c308468818a389519d19201d5e0da2f5ba28743e3b79cc5a
  H=4a561d26ebda75c180adf8f41ff2776b1952b272b93baf7d2e6f4d323f02d295
  I=ddf42dcda9d9d37ad7e1f02a48a6887b85c6632d7df4e915c67a1ebf340b4a06
  J=87369e5f4de3a683e1f06ba55b46a126984be570238977edf5e9253dce536206
  K=247baaa2f5cbd31861f6005be1a3e33ef9fb4bfd2a8b3c5221152267c663b396
  L=4fd4c4ba074425ed1bc68c1058e71098fe8a9a4379d4c4010a0637fb48e76655
  M=9b1f47161fffde19746e86064907d9c9ce67e8460df6354ed2a249e2988d2547
  N=e740c4212c22b02e2e0537285aed6e3e18b6cfbf64930a6f9d08b6575268f453
  O=f8fb2925cf0ed23caf28b596547a39dfc66e39fea9c74f506d5323f5c6c00189
  P=93fb02ab5bc2a5ce4fb47586a818e589bff242d4f38acf171132a936e3b483b7
  Q=457604f9b00fe70d661363457439230ed7388ac75c8888dc71b95d5374d87553
  R=fded11d5c4c4f8bd7285e7c7a8c84feb8a6a46b264cd44c0ef4a7121d45bc813
  S=df61a91ffdfa241438f1f65ca7303258b8a04f66bf1ab4088015bfc85a463b6e
  T=6bc1e9e3124f50afeab93167816e1c062a85e965739c3a67e02526555b0ca357
  U=191f1f26c7a5a974a1f453e4126046930a253305eaf24aed0b5d07ef0afc3d2a
  V=4720f0b263a03f20d2559b4fb29a518b311f57401faf86eaf8658a565efd534c
  W=72706786c3e15ccc27917c68e0d5be053295585213f11748bec9ac1440d421e5
  X=8d0a4b7a721c73fc712685487738bb7f1f3d0ccfaecbefbe3f0fdb2b7be78f94
  Y=fe4d606906cf246e9dba2875cf11e7a0c20402c85f9a1c5064fbabf51048be85
  Z=6b84580e28920fc0350d124319034b4f56a60c75625258601673c3011ada959f

The graph can be extended with more commits.  The node names don't
need to match the previous graph (although it's probably a good idea).

  $ mononoke_testtool drawdag -R repo <<'EOF'
  >        XX    # modify: XX path/to/file "more additional content"
  >       /  \   # bookmark: XX xxxx
  >     D1    W2
  >     |     |
  >     |     W1
  >     D0    |  # exists: D0 971c152b1c5d22763033d2fc23ac40c1e785a3c50df28e37b69bf188f2d949b5
  >           W0 # exists: W0 72706786c3e15ccc27917c68e0d5be053295585213f11748bec9ac1440d421e5
  > EOF
  *] Reloading redacted config from configerator (glob)
  D0=971c152b1c5d22763033d2fc23ac40c1e785a3c50df28e37b69bf188f2d949b5
  D1=5d656cad8da7be07bf90202f578bd9855688da10bed44fd2b3d7f62e241abe75
  W0=72706786c3e15ccc27917c68e0d5be053295585213f11748bec9ac1440d421e5
  W1=59132264e74b167e7429fc2dab693113fb14fb2a07a94a1cd2243697b06f8aee
  W2=0dadf010323f9f62459caba47668b50d92e1a0c05856c4c3adb6c042558c91d0
  XX=c680ff4353433b1e77b110541fdf492f9847d95b6805dc8880072709caefd461
