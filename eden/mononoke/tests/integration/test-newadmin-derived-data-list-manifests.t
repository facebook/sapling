# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ testtool_drawdag -R repo <<'EOF'
  > A-B-C
  > # modify: A "a/foo.txt" "a_foo"
  > # modify: A "a/bar.txt" "a_bar"
  > # modify: B "a/b/bar.txt" "b_bar"
  > # modify: B "b/hoo.txt" "b_hoo"
  > # delete: B "a/bar.txt"
  > # modify: C "a/b/c/foo.txt" "c_foo"
  > # modify: C "a/b/c/hoo.txt" "c_hoo"
  > # delete: C "b/hoo.txt"
  > # bookmark: C main
  > EOF
  A=ee3f74b0fc3e4862c21cb6dc6ac90901072e48d6c863bd4413c0ca660a16e1d9
  B=b65c0e6f73c666e4f7b9b4bdddfcb72f2c8beef5968bbfc13ed1b231536f8e11
  C=0b95b6947772ea75083a16af5c9cdc2c3f76b23c26c834f0bdfe227819319a2b

derived-data list-manifests:

Skeleton manifests
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifests -p "a" -B main skeleton-manifests
  a/b/
  a/foo.txt
Skeleton manifests from root
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifests -p "" -i "$B" skeleton-manifests
  A
  B
  a/
  b/
Skeleton manifests (recursive)
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifests -p "a" -i "$B" skeleton-manifests --recursive
  a/foo.txt
  a/b/bar.txt
Skeleton manifests from root (recursive)
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifests -p "" -B main skeleton-manifests --recursive
  A
  B
  C
  a/foo.txt
  a/b/bar.txt
  a/b/c/foo.txt
  a/b/c/hoo.txt
Fsnodes
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifests -p "a" -B main fsnodes
  a/foo.txt	67f9f510b6a13f94986928ba0f270ec005b194edd77b22a13dec797471a4fe85	regular	5
  a/b/bar.txt	638aceddb6283739ca98ac2cb18bf6d8d5358439ea187fd4ab0257d24d6d6e47	regular	5
  a/b/c/foo.txt	a2ad79422b22799f40b07486efbe522add2d31b7ebd809989a20d74fea833684	regular	5
  a/b/c/hoo.txt	3ce7f4c533d5a93f131f1f7dc6f887642d5da12e47496afaa589e5aabb29fa8a	regular	5

Fsnodes from root path
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifests -p "" -i "$B" fsnodes
  A	eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9	regular	1
  B	55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f	regular	1
  a/foo.txt	67f9f510b6a13f94986928ba0f270ec005b194edd77b22a13dec797471a4fe85	regular	5
  a/b/bar.txt	638aceddb6283739ca98ac2cb18bf6d8d5358439ea187fd4ab0257d24d6d6e47	regular	5
  b/hoo.txt	88c50336ada15d8abe61f2adce8af17b63eb74985d50eec76d4d0248f33bb4a9	regular	5
Unodes
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifests -p "a" -B main unodes
  a/ ManifestUnodeId(Blake2(dbdbdd1b393b32741aaab820850468c06b3f50319bea3728e8b2d346e61a01ef))
  a/foo.txt FileUnodeId(Blake2(5b5ddd33b0347715e192bfc25bc172ed8c5800d87ba3d3238ef88dee25d28dc6))
  a/b/ ManifestUnodeId(Blake2(48caf3edd514179ebde2bec7cc44bbc1e925b633a232eb9672fce099ca09054b))
  a/b/bar.txt FileUnodeId(Blake2(4e8fbca02d5fa0d2a9abb7f075d8b5c4ad22e54e49dd6e18e00590032b1d3064))
  a/b/c/ ManifestUnodeId(Blake2(4ae4e944e4e9f66a46e41eeb20cdc3631711cab8c07bba3714f0753264b5453b))
  a/b/c/foo.txt FileUnodeId(Blake2(a1aedf1fea5759d59f02e22cf8a6036c151ff1da913cf9215805cc55a1dc8a68))
  a/b/c/hoo.txt FileUnodeId(Blake2(eb6aaff4bf9645666875dfd18d6407ccd7a153de2fcbf64d4d7ae883cf07a2bb))

Unodes from root
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifests -p "" -i "$B" unodes
  / ManifestUnodeId(Blake2(8ade1b6151194edff398e823450e3bfbc8a1252958ea89f0c3ef58b0c3d30e70))
  A FileUnodeId(Blake2(5da8409b6ec0f3759444f93c2c5194f5c94c02037095ca16b5f3e0f70152c613))
  B FileUnodeId(Blake2(eb68a776a3017fcc811f6f23a8724a771db09de2f35fda2db314b580d41fb7ae))
  a/ ManifestUnodeId(Blake2(abb223a8d49252e82a934e09c8031ce77dd5fe70d481aace567b6f0b13e90e95))
  a/foo.txt FileUnodeId(Blake2(5b5ddd33b0347715e192bfc25bc172ed8c5800d87ba3d3238ef88dee25d28dc6))
  a/b/ ManifestUnodeId(Blake2(0d78d5210f51fcf3f7dd906722f1a0080ad4062b71c32a811b13edec903eeb06))
  a/b/bar.txt FileUnodeId(Blake2(4e8fbca02d5fa0d2a9abb7f075d8b5c4ad22e54e49dd6e18e00590032b1d3064))
  b/ ManifestUnodeId(Blake2(4668835e236dfec9b0273f21f33cfa4570769a24a5a5422b10920b28e5440092))
  b/hoo.txt FileUnodeId(Blake2(54942dc4ea2bd38839a40566d01e06d56e479adaeba9b3c64b94e55ae6911936))

Deleted manifests
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifests -p "a" -i "$B" deleted-manifests
  a/bar.txt/ DeletedManifestV2Id(Blake2(fa523e73a133223c61a827b226f8e339e136957ff48d7614d55dd0e18a42c19d))

Deleted manifests from root
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifests -p "" -B "main" deleted-manifests
  b/ DeletedManifestV2Id(Blake2(c9e91618b8e2c37c1ead087030945e4feaa7adabe24b93aa7e41ed1de9ce6b88))
  b/hoo.txt/ DeletedManifestV2Id(Blake2(f67b8e1fe09de8ccc5697cbe4290bac4af2a889b03fb1d04a145c3c032bd865b))
  a/bar.txt/ DeletedManifestV2Id(Blake2(fa523e73a133223c61a827b226f8e339e136957ff48d7614d55dd0e18a42c19d))
