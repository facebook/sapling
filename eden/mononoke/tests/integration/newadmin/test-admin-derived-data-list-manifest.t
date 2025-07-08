# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ ADDITIONAL_DERIVED_DATA="content_manifests" setup_common_config
  $ testtool_drawdag -R repo <<'EOF'
  > A-B-C
  > # modify: A "a/foo.txt" "a_foo"
  > # modify: A "a/bar.txt" "a_bar"
  > # modify: A "script.sh" exec "test"
  > # modify: B "a/b/bar.txt" "b_bar"
  > # modify: B "b/hoo.txt" "b_hoo"
  > # delete: B "a/bar.txt"
  > # modify: B "script" link "script.sh"
  > # modify: C "a/b/c/foo.txt" "c_foo"
  > # modify: C "a/b/c/hoo.txt" "c_hoo"
  > # delete: C "b/hoo.txt"
  > # bookmark: C main
  > EOF
  A=734dc23869fbed1c81d6561a16f0e896aa73bf037e688562c6bad691368db9fa
  B=581ea2acc78e89f96ece88fc87956018ffd01941d62119e055bbc9348d98caad
  C=3371afd62725ca00669b19e45fed925030a601c26c409742583da2cf3c5e6eae

derived-data list-manifest:

Skeleton manifest of main's a directory
  $ mononoke_admin derived-data -R repo list-manifest -p "a" -B main -t skeleton-manifests --derive
  a/b/	32d74c40e3ed0a76b1fe09ee7251df9805370e271b4a815ce08a96b640228b61
  a/foo.txt	exists
Skeleton manifest of B's root directory
  $ mononoke_admin derived-data -R repo list-manifest -i "$B" -t skeleton-manifests
  A	exists
  B	exists
  a/	02d87d7d93a5072f4fe981d3801b13d6ca4157ad1387ffbeb20363463b19ff9a
  b/	8cd7d51ac1beaec4c16067a6f91ad5140754e3c07013ae939db474ae947afb6b
  script	exists
  script.sh	exists
Skeleton manifest of B's a directory (recursive)
  $ mononoke_admin derived-data -R repo list-manifest -p "a" -i "$B" -t skeleton-manifests --recursive | sort
  a/b/bar.txt	exists
  a/foo.txt	exists
Skeleton manifest of main's root directory (recursive)
  $ mononoke_admin derived-data -R repo list-manifest -B main -t skeleton-manifests --recursive | sort
  A	exists
  B	exists
  C	exists
  a/b/bar.txt	exists
  a/b/c/foo.txt	exists
  a/b/c/hoo.txt	exists
  a/foo.txt	exists
  script	exists
  script.sh	exists
  $ mononoke_admin derived-data -R repo list-manifest -B main -t skeleton-manifests2 --derive | sort
  A	file
  B	file
  C	file
  a/	tree	count=7
  script	file
  script.sh	file
  $ mononoke_admin derived-data -R repo list-manifest -B main -p "a" -t skeleton-manifests2 --derive | sort
  a/b/	tree	count=5
  a/foo.txt	file

Fsnodes of main's a directory
  $ mononoke_admin derived-data -R repo list-manifest -p "a" -B main -t fsnodes --derive | sort
  a/b/	f43b8e1f3b620c61eb8e47329df9cee895b77613f2afeb1d53238c63ebba58c4
  a/foo.txt	67f9f510b6a13f94986928ba0f270ec005b194edd77b22a13dec797471a4fe85	type=regular	size=5
Fsnodes from B's root path (recursive)
  $ mononoke_admin derived-data -R repo list-manifest -i "$B" -t fsnodes --recursive | sort
  A	eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9	type=regular	size=1
  B	55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f	type=regular	size=1
  a/b/bar.txt	638aceddb6283739ca98ac2cb18bf6d8d5358439ea187fd4ab0257d24d6d6e47	type=regular	size=5
  a/foo.txt	67f9f510b6a13f94986928ba0f270ec005b194edd77b22a13dec797471a4fe85	type=regular	size=5
  b/hoo.txt	88c50336ada15d8abe61f2adce8af17b63eb74985d50eec76d4d0248f33bb4a9	type=regular	size=5
  script	f3fffae72590e3c9b4bd8801665ac3c9e16f35c63ba77c4642a54e1c0ad1d3f8	type=symlink	size=9
  script.sh	7944a589808e894931ed482c1cb0543524483a49aaf9568e60959a34fe9700d9	type=executable	size=4

Content manifests
  $ mononoke_admin derived-data -R repo list-manifest -p "a" -B main -t content-manifests --derive | sort
  a/b/	6c6855704970b38c87329e762932ff95eebcfb2b60ec2e93150f6b5270b42e1f
  a/foo.txt	67f9f510b6a13f94986928ba0f270ec005b194edd77b22a13dec797471a4fe85	type=regular	size=5
Content manifests from root path, recursive
  $ mononoke_admin derived-data -R repo list-manifest -p "" -B main -t content-manifests --recursive | sort
  A	eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9	type=regular	size=1
  B	55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f	type=regular	size=1
  C	896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d	type=regular	size=1
  a/b/bar.txt	638aceddb6283739ca98ac2cb18bf6d8d5358439ea187fd4ab0257d24d6d6e47	type=regular	size=5
  a/b/c/foo.txt	a2ad79422b22799f40b07486efbe522add2d31b7ebd809989a20d74fea833684	type=regular	size=5
  a/b/c/hoo.txt	3ce7f4c533d5a93f131f1f7dc6f887642d5da12e47496afaa589e5aabb29fa8a	type=regular	size=5
  a/foo.txt	67f9f510b6a13f94986928ba0f270ec005b194edd77b22a13dec797471a4fe85	type=regular	size=5
  script	f3fffae72590e3c9b4bd8801665ac3c9e16f35c63ba77c4642a54e1c0ad1d3f8	type=symlink	size=9
  script.sh	7944a589808e894931ed482c1cb0543524483a49aaf9568e60959a34fe9700d9	type=executable	size=4

Unodes of main's a directory
  $ mononoke_admin derived-data -R repo list-manifest -p "a" -B main -t unodes --derive
  a/b/	102bf16d65a69acdfc009c57dcb04a5320793d4127f3380a563f4321dec5e188
  a/foo.txt	6ff43b2e8ed1fe11cb9d4960b3b98b2b6b74f8d33b07212b21e703a26bff7bab

Unodes of B's root (recursive)
  $ mononoke_admin derived-data -R repo list-manifest -i "$B" -t unodes --recursive | sort
  A	cd771475fbda7931a732013c817545b570f2fda7aedd5ee15677168c54e713b6
  B	670de42024de2d059cc795e4af983511e283f7a20edac3f2c07954dde321133c
  a/b/bar.txt	d865bc1be52ba5b788200e52fc52e091cee771ada2db4b8dd2e5360e181775df
  a/foo.txt	6ff43b2e8ed1fe11cb9d4960b3b98b2b6b74f8d33b07212b21e703a26bff7bab
  b/hoo.txt	1ce5ae2c91abb6b29783861478a5bc65df2e6220f9c292e4fe4bcec94fddfa06
  script	6b4739b1309ad708365d3115916f0979f36dae4a852dd8263c554a177e5dbcd9
  script.sh	8bdc9692cef408f0067f89ae55b0f3dab8e74ce8f63e34137ea5ef944e309dfe

Deleted manifests of B's a directory
  $ mononoke_admin derived-data -R repo list-manifest -p "a" -i "$B" -t deleted-manifests --derive | sort
  a/bar.txt	2d424b26533fbe5aafdfb7a7f9834282630b465b2b1b56194c9e0689df8ec2f2	linknode=581ea2acc78e89f96ece88fc87956018ffd01941d62119e055bbc9348d98caad

Deleted manifests of main from root (recursive)
Note that `b/` appears because the directory was fully deleted.
  $ mononoke_admin derived-data -R repo list-manifest -B "main" -t deleted-manifests --derive --recursive | sort
  a/bar.txt	2d424b26533fbe5aafdfb7a7f9834282630b465b2b1b56194c9e0689df8ec2f2	linknode=581ea2acc78e89f96ece88fc87956018ffd01941d62119e055bbc9348d98caad
  b/	0646f87ccb30da2fdfdc22c453d9bdb5a75e25d3104a8a1b25cc48037fd21cf6	linknode=3371afd62725ca00669b19e45fed925030a601c26c409742583da2cf3c5e6eae
  b/hoo.txt	c14cdb955b6ec1ce0f3b76e80d9aaca4e86766e08025e5b286dfeadc150d9b20	linknode=3371afd62725ca00669b19e45fed925030a601c26c409742583da2cf3c5e6eae

#  $ mononoke_admin derived-data -R repo derive -B "main" -T git_commits

  $ mononoke_admin derived-data -R repo list-manifest -B "main" -t git-trees --derive --recursive | sort
  A	8c7e5a667f1b771847fe88c01c3de34413a1b220	mode=100644
  B	7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54	mode=100644
  C	96d80cd6c4e7158dbebd0849f4fb7ce513e5828c	mode=100644
  a/b/bar.txt	b31036e311273a931b70aeb7f79d4f30a22d1194	mode=100644
  a/b/c/foo.txt	aa16822950ad21d28d240f73b69085ec2aaedb64	mode=100644
  a/b/c/hoo.txt	60f4e4ce701de2916bf7d19765260b21e70e49a1	mode=100644
  a/foo.txt	a4307d05df50a4b31e6b6b9da8c5921ffafbae96	mode=100644
  script	0231def3d8f55958dddba757de918ca5eae0df4c	mode=120000
  script.sh	30d74d258442c7c65512eafab474568dd706c430	mode=100755

Validate all these manifests are equivalent
  $ mononoke_admin derived-data -R repo verify-manifests -i "$A" -T fsnodes -T hgchangesets -T unodes -T skeleton_manifests -T git_commits -T content_manifests
  $ mononoke_admin derived-data -R repo verify-manifests -i "$B" -T fsnodes -T hgchangesets -T unodes -T skeleton_manifests -T git_commits -T content_manifests
  $ mononoke_admin derived-data -R repo verify-manifests -i "$C" -T fsnodes -T hgchangesets -T unodes -T skeleton_manifests -T git_commits -T content_manifests
