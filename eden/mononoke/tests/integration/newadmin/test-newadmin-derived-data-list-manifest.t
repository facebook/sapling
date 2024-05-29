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

derived-data list-manifest:

Skeleton manifest of main's a directory
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifest -p "a" -B main -t skeleton-manifests --derive
  a/b/	32d74c40e3ed0a76b1fe09ee7251df9805370e271b4a815ce08a96b640228b61
  a/foo.txt	exists
Skeleton manifest of B's root directory
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifest -i "$B" -t skeleton-manifests
  A	exists
  B	exists
  a/	02d87d7d93a5072f4fe981d3801b13d6ca4157ad1387ffbeb20363463b19ff9a
  b/	8cd7d51ac1beaec4c16067a6f91ad5140754e3c07013ae939db474ae947afb6b
Skeleton manifest of B's a directory (recursive)
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifest -p "a" -i "$B" -t skeleton-manifests --recursive | sort
  a/b/bar.txt	exists
  a/foo.txt	exists
Skeleton manifest of main's root directory (recursive)
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifest -B main -t skeleton-manifests --recursive | sort
  A	exists
  B	exists
  C	exists
  a/b/bar.txt	exists
  a/b/c/foo.txt	exists
  a/b/c/hoo.txt	exists
  a/foo.txt	exists

Fsnodes of main's a directory
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifest -p "a" -B main -t fsnodes --derive | sort
  a/b/	f43b8e1f3b620c61eb8e47329df9cee895b77613f2afeb1d53238c63ebba58c4
  a/foo.txt	67f9f510b6a13f94986928ba0f270ec005b194edd77b22a13dec797471a4fe85	type=regular	size=5
Fsnodes from B's root path (recursive)
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifest -i "$B" -t fsnodes --recursive | sort
  A	eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9	type=regular	size=1
  B	55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f	type=regular	size=1
  a/b/bar.txt	638aceddb6283739ca98ac2cb18bf6d8d5358439ea187fd4ab0257d24d6d6e47	type=regular	size=5
  a/foo.txt	67f9f510b6a13f94986928ba0f270ec005b194edd77b22a13dec797471a4fe85	type=regular	size=5
  b/hoo.txt	88c50336ada15d8abe61f2adce8af17b63eb74985d50eec76d4d0248f33bb4a9	type=regular	size=5

Unodes of main's a directory
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifest -p "a" -B main -t unodes --derive
  a/b/	48caf3edd514179ebde2bec7cc44bbc1e925b633a232eb9672fce099ca09054b
  a/foo.txt	5b5ddd33b0347715e192bfc25bc172ed8c5800d87ba3d3238ef88dee25d28dc6

Unodes of B's root (recursive)
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifest -i "$B" -t unodes --recursive | sort
  A	5da8409b6ec0f3759444f93c2c5194f5c94c02037095ca16b5f3e0f70152c613
  B	eb68a776a3017fcc811f6f23a8724a771db09de2f35fda2db314b580d41fb7ae
  a/b/bar.txt	4e8fbca02d5fa0d2a9abb7f075d8b5c4ad22e54e49dd6e18e00590032b1d3064
  a/foo.txt	5b5ddd33b0347715e192bfc25bc172ed8c5800d87ba3d3238ef88dee25d28dc6
  b/hoo.txt	54942dc4ea2bd38839a40566d01e06d56e479adaeba9b3c64b94e55ae6911936

Deleted manifests of B's a directory
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifest -p "a" -i "$B" -t deleted-manifests --derive | sort
  a/bar.txt	fa523e73a133223c61a827b226f8e339e136957ff48d7614d55dd0e18a42c19d	linknode=b65c0e6f73c666e4f7b9b4bdddfcb72f2c8beef5968bbfc13ed1b231536f8e11

Deleted manifests of main from root (recursive)
Note that `b/` appears because the directory was fully deleted.
  $ with_stripped_logs mononoke_newadmin derived-data -R repo list-manifest -B "main" -t deleted-manifests --derive --recursive | sort
  a/bar.txt	fa523e73a133223c61a827b226f8e339e136957ff48d7614d55dd0e18a42c19d	linknode=b65c0e6f73c666e4f7b9b4bdddfcb72f2c8beef5968bbfc13ed1b231536f8e11
  b/	c9e91618b8e2c37c1ead087030945e4feaa7adabe24b93aa7e41ed1de9ce6b88	linknode=0b95b6947772ea75083a16af5c9cdc2c3f76b23c26c834f0bdfe227819319a2b
  b/hoo.txt	f67b8e1fe09de8ccc5697cbe4290bac4af2a889b03fb1d04a145c3c032bd865b	linknode=0b95b6947772ea75083a16af5c9cdc2c3f76b23c26c834f0bdfe227819319a2b

Validate all these manifests are equivalent
  $ with_stripped_logs mononoke_newadmin derived-data -R repo verify-manifests -i "$A" -T fsnodes -T hgchangesets -T unodes -T skeleton_manifests
  $ with_stripped_logs mononoke_newadmin derived-data -R repo verify-manifests -i "$B" -T fsnodes -T hgchangesets -T unodes -T skeleton_manifests
  $ with_stripped_logs mononoke_newadmin derived-data -R repo verify-manifests -i "$C" -T fsnodes -T hgchangesets -T unodes -T skeleton_manifests
