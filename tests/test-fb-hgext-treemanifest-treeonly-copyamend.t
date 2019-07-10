TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
  $ setconfig treemanifest.flatcompat=False
  $ . "$TESTDIR/library.sh"

Setup the server

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > treemanifest=
  > [treemanifest]
  > server=True
  > treeonly=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

Make local commits on the server for a file in a deep directory with a long
history, where the new file content is introduced on a separate branch each
time.
  $ mkdir -p a/b/c/d/e/f/g/h/i/j
  $ echo "base" > a/b/c/d/e/f/g/h/i/j/file
  $ hg commit -qAm "base"
  $ for i in 1 2 3 4 5 6 7 8 9 10 11 12
  > do
  >   echo $i >> a/b/c/d/e/f/g/h/i/j/file
  >   echo $i >> a/b/c/d/e/f/g/h/i/otherfile$i
  >   hg commit -qAm "commit $i branch"
  >   hg up -q ".^"
  >   echo $i >> a/b/c/d/e/f/g/h/i/j/file
  >   echo $i >> a/b/c/d/e/f/g/h/i/otherfile$i
  >   hg commit -qAm "commit $i"
  > done

  $ hg log -G -r 'all()' -T '{rev} {desc}'
  @  24 commit 12
  |
  | o  23 commit 12 branch
  |/
  o  22 commit 11
  |
  | o  21 commit 11 branch
  |/
  o  20 commit 10
  |
  | o  19 commit 10 branch
  |/
  o  18 commit 9
  |
  | o  17 commit 9 branch
  |/
  o  16 commit 8
  |
  | o  15 commit 8 branch
  |/
  o  14 commit 7
  |
  | o  13 commit 7 branch
  |/
  o  12 commit 6
  |
  | o  11 commit 6 branch
  |/
  o  10 commit 5
  |
  | o  9 commit 5 branch
  |/
  o  8 commit 4
  |
  | o  7 commit 4 branch
  |/
  o  6 commit 3
  |
  | o  5 commit 3 branch
  |/
  o  4 commit 2
  |
  | o  3 commit 2 branch
  |/
  o  2 commit 1
  |
  | o  1 commit 1 branch
  |/
  o  0 base
  
Create a client
  $ hgcloneshallow ssh://user@dummy/master client -q --config treemanifest.treeonly=True --config extensions.treemanifest=
  fetching tree '' efa8fa4352b919302f90e85924e691a632d6bea0, found via 9f95b8f1011f
  11 trees fetched over *s (glob)
  13 files fetched over *s (glob)
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [experimental]
  > evolution = createmarkers, allowunstable
  > [extensions]
  > amend=
  > fastmanifest=
  > treemanifest=
  > [treemanifest]
  > sendtrees=True
  > treeonly=True
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > [remotefilelog]
  > reponame=treeonlyrepo
  > EOF

Rename the file in a commit
  $ hg mv a/b/c/d/e/f/g/h/i/j/file a/b/c/d/e/f/g/h/i/j/file2
  $ hg commit -m "rename"
  fetching tree '' efa8fa4352b919302f90e85924e691a632d6bea0, found via 9f95b8f1011f
  11 trees fetched over *s (glob)
  * files fetched over *s (glob)

Amend the commit to add a new file with an empty cache, with descendantrevfastpath enabled
  $ clearcache
  $ echo more >> a/b/c/d/e/f/g/h/i/j/file3
  $ hg amend -A --config remotefilelog.debug=True --config remotefilelog.descendantrevfastpath=True
  adding a/b/c/d/e/f/g/h/i/j/file3
  fetching tree '' efa8fa4352b919302f90e85924e691a632d6bea0, found via 9f95b8f1011f
  11 trees fetched over *s (glob)
  fetching tree '' c36ca99af86631de37bd6c95d8cfe94d3ce16754, based on efa8fa4352b919302f90e85924e691a632d6bea0, found via 83bc02216909
  1 trees fetched over *s (glob)
  fetching tree 'a' 7ba74f864b7769e9a50114bc898fd33845e2702a
  1 trees fetched over *s (glob)
  fetching tree 'a/b' 25df1d2a11010ec3166546b9caef67a3943fa6bd
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c' 1a136a5502f8a2dc6529e9e4ff14730e76581043
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d' 30f21d3e98d37a7dd47f209e043b550097af82e7
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e' 4a4af8cad180e4b3070387d7ac2b29ca89dc19c7
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f' d61a2f5766233cf6059c6132ae81e50348551320
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g' ca017dd54e22be982c6889b1a61015c57066cf19
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h' 024793a97d7d5c5c4446e33bbf80c2c6755d3db6
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i' 848c4f313952702d080d3476baab7d20dd9de06e
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i/j' 1284a2aa8aa0a30ecec7ca3b221358d4df9a5cfa
  1 trees fetched over *s (glob)
  fetching tree '' 51993ae18844f04f4799689d34b2ac5ae709c827, based on efa8fa4352b919302f90e85924e691a632d6bea0, found via 83bc02216909
  1 trees fetched over *s (glob)
  fetching tree 'a' 443b5652089f73552f9176d5dc82e3e36987052d
  1 trees fetched over *s (glob)
  fetching tree 'a/b' d8fcf43c75a5410c9f979474c5e3ca3b988cca13
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c' 7d42cf708cbcbe6f9ae4cf56f522f200d25860d4
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d' 454865aeebbc66a4b7b010a84dd70078c1d4a83e
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e' edcd8e2ef4d3100c1a1078b5afce8b0f250cbd68
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f' 7449200f6ef112b0b20fdfbf888c2afcce0335b9
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g' 8cb8a455ed3f8087b96d160a86237988625d24ef
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h' 173dfa704cd88c6f192047d708afecd6ee8982b8
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i' 1a6fb5d11c732a27f76d92715605f53b8d0ba2a6
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i/j' a5c017fef7b52316f618acc26070fa7ca7bb413e
  1 trees fetched over *s (glob)
  fetching tree '' af5d2a10e1dc973436f536e246cb6d1c5631b675, based on efa8fa4352b919302f90e85924e691a632d6bea0, found via 83bc02216909
  1 trees fetched over *s (glob)
  fetching tree 'a' b6f3087f47bd9d6f7b78575f641b4b7b5bf31f3d
  1 trees fetched over *s (glob)
  fetching tree 'a/b' b699f3870c6ad17278ef53b45c7bbc161e76b338
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c' 3c4edac7d68075d4ac8112e590bd85e001ee5530
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d' 4a277b394f6d1a2480efa90bc21c12e1ec04b32c
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e' bc63c2fb3db18b78df3c3b4a597a82099ad01d87
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f' 8c97e5458ab78fbf219dabfa0dc429ea825532f0
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g' b6c9d12e8029af045817534e944c7b825e887ba0
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h' 04f5492270a5f6f739ffac9f50ddaef39cb5f230
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i' 53f58cb37f65acde1fea1a789b2112813e29cb22
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i/j' bebca98a7037679a2bd2acd72595aaefad440767
  1 trees fetched over *s (glob)
  fetching tree '' 40e8e549358bc9fe2635595849667ffe671001f4, based on efa8fa4352b919302f90e85924e691a632d6bea0, found via 83bc02216909
  1 trees fetched over *s (glob)
  fetching tree 'a' 2529ad42a92a7a5a38784c987cd7c973e8192905
  1 trees fetched over *s (glob)
  fetching tree 'a/b' 314edeabd5f64afd12ace49b777bd10097127854
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c' 926e44d12fdcd59423c754411662872da23e46e9
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d' c6a54c04c5f6a73f8fc49e2263fd9e0c5aad783d
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e' 6b444e48d1e9f955ce46419d545d45bc873dffff
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f' e387f64bcc3d24974eb46c16cffc50ae26533d5b
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g' bd0b718f50b32544789639b6ab596c2cbd2374ee
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h' 4a4a5921a4e7be8ad826091a4209fe5c05d66059
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i' df79804e75b5be1f8416f1633a4f3ca930e32f09
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i/j' c8222170d15eaadd49e9ef78f4712b476b7c7a1d
  1 trees fetched over *s (glob)
  fetching tree '' b0056e33a093404df30fd129119441c97a776206, based on efa8fa4352b919302f90e85924e691a632d6bea0, found via 83bc02216909
  1 trees fetched over *s (glob)
  fetching tree 'a' f8cf453ce2194bbb5da76dedb2ef91c504f7f815
  1 trees fetched over *s (glob)
  fetching tree 'a/b' a74f42df47c426214afc563649ec067adb83b7e7
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c' af2cd120c32232288943aa71abbf930fe15d86da
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d' 4a4f879285d3d4ede893f0bfd15e4fcd639fdbf8
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e' bb1d06a7b77337d3f22c06b8591d643adf463940
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f' 8789e4ba9da2bfa846f8b55e3c0637745e72c232
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g' 43a07a886a4dc4f922d8020a0bd8e87d1294853f
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h' 098e9a68be9bc40f9a9e0dd3d6beac196a7effef
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i' 1295f6b43270db45eda5e6f83e724f526b5006c2
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i/j' 7b443ef6e15ef5a96729a99d5644bc4395e5e9e7
  1 trees fetched over *s (glob)
  fetching tree '' 7514f4dfe9163536f425e2441490a9ce67d9c9b7, based on efa8fa4352b919302f90e85924e691a632d6bea0, found via 83bc02216909
  1 trees fetched over *s (glob)
  fetching tree 'a' c99eb146990f3e50b37f3f495c53f40a2cb326aa
  1 trees fetched over *s (glob)
  fetching tree 'a/b' 66d9e3e99dd6cce8d935eabc519a31a90d9d09b9
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c' 15f749bc02ba152669851008e82d10ff8f93670f
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d' ad796b4d165cae581b88096a33516dde67f12aeb
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e' 5ed1e26dde0de6c6a1818d54c6f0a68663074044
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f' 5d4ef48d1f82a26a68c2f549f3567834e434588f
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g' a6d0eb138d2e6ba5b121aaae84bff40123006df8
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h' 9b3235c12dbf3738a0a64323d0863a10d72563e5
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i' 2a2238161cc7ec666d721480a9e6e214dcd70a28
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i/j' c59321f2b812dd0468ead0ed6ee64053366750e6
  1 trees fetched over *s (glob)
  fetching tree '' 1a75ce1fc7ce8c282d68feb463d9001ef711af03, based on efa8fa4352b919302f90e85924e691a632d6bea0, found via 83bc02216909
  1 trees fetched over *s (glob)
  fetching tree 'a' df05c56a449f412cfbb0c82a1bdd4a867baa4cf8
  1 trees fetched over *s (glob)
  fetching tree 'a/b' b3b62fe280fa4008c2774c7664911916d4a51bca
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c' 9a975f5d3c647c919643d09c259fa9d215afd31d
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d' 93e36226a5b8a67bbfd5897083dd390ca0c82a98
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e' e3ecf91a79164e87f999ed06277a75fc2616bc8b
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f' 04d72fc51fde9806327145cfb96fc3eb0fe301f6
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g' 27c535decf5b1422ce78afad7599d478ed587be6
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h' 8ddadfa02827c1f16539157ff19ba6851ad89f7f
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i' 38b1b188ef352e2908f3cd9409b82a7f8ce5c751
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i/j' a1380f14c6c0aca48db8935b16c6d927632fcd7a
  1 trees fetched over *s (glob)
  fetching tree '' 38b576c2c47a096aa7992fa56bb66d6198dcd8e8, based on efa8fa4352b919302f90e85924e691a632d6bea0, found via 83bc02216909
  1 trees fetched over *s (glob)
  fetching tree 'a' 302d1a231c8c0ddab8d51a9a9d1e93c49eceeed8
  1 trees fetched over *s (glob)
  fetching tree 'a/b' 2a9665f20203434b58bfd71c6e81a518e2c69ec6
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c' 900ba5f3f6ed281c03423176ee3c09b9204bbc94
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d' 22a43e9cd99633dc07f1527659c10d889de9326e
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e' 276cce07c7b6fc76d2c09fe54b4d1861be684338
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f' 87d2c43ffbd3d4f41d5e5b031fd543dc62282eea
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g' 2ad7d88f5c335518815941af6a01dc2ae5455466
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h' 84f99762378db674bce383271e7f1af64172a2c6
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i' dac94b3e8a8477fe7f2e17142af2c33378f76a25
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i/j' 11cfc67827bf35cf809258ab718e4587ca2b792a
  1 trees fetched over *s (glob)
  fetching tree '' 1fb85cd8fcb08c202f1782a5dbc84a3f727bb049, based on efa8fa4352b919302f90e85924e691a632d6bea0, found via 83bc02216909
  1 trees fetched over *s (glob)
  fetching tree 'a' aa4886e0996a3ac01bf598b65bacfb91d3c4bd94
  1 trees fetched over *s (glob)
  fetching tree 'a/b' dae96a1f0295df372abed901f76e761a32de28f6
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c' 1f609db1922bd1fe0a8eb627dfa093b3b42770e3
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d' 9779e6ef31141186fdc74fd59e735da190cdaf6e
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e' 187baca213c270c47a1532b77be0575ccd46cc19
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f' 046101bdda4c82235705ffc14057103b906d2a17
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g' 053cb66e96611888a70650d9649ca6370877c583
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h' 89bb277fda5780a801966e2e5413e86d03ff9f39
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i' 1b1964f393ab012e7980ccd08faba1184a51f5f4
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i/j' 608edb31809fa68cd51cd844ccb7802907dbff54
  1 trees fetched over *s (glob)
  fetching tree '' c73e4dc5ff17f1ff107be7403baad61e6b8942e6, based on efa8fa4352b919302f90e85924e691a632d6bea0, found via 83bc02216909
  1 trees fetched over *s (glob)
  fetching tree 'a' 01b86bfdf6952c0f2460a5d92f6042930029bd28
  1 trees fetched over *s (glob)
  fetching tree 'a/b' eafa952c77ba3f082ff5889405ba3fb06df5a72a
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c' cc6bea909ccc300744384099988f5ba726d942f2
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d' fa2706e73c7b139178cfddab311aaeff81c03442
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e' 148de6c40001ae2a6e22f799162f7cc46fe954ee
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f' 72a5afeecc448292376e4bf528a976a10b339092
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g' fe01de3c9f299d989a727593d5703847c1b5c5a1
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h' 35b0d3bc1c901e61619966afa12b5e68ef634cdd
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i' db7ca0d7bb3962444e83d2883b4a1aa53fb703ec
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i/j' fac503b2f71e494bdc1fc1f5c6ebf76e284331a3
  1 trees fetched over *s (glob)
  fetching tree '' 52dad11d735c91890d5150d6dfaee135cd807f62, based on efa8fa4352b919302f90e85924e691a632d6bea0, found via 83bc02216909
  1 trees fetched over *s (glob)
  fetching tree 'a' edd749fc53e9eecc210b0958d2b97f4450e81b29
  1 trees fetched over *s (glob)
  fetching tree 'a/b' 67b5bc7cadc692086f080b56890a2433119a2d5c
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c' bd7ce97f0bd805b3c72b390cdf3d7d0c47baefed
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d' 69481ea0dc677c00445913b56d3d4f3f80588e61
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e' 1a6d74eed9a1c4c0ee4a62e83d7351ae712b8aa2
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f' db0a0f13d8a3cc9461e287472417cf48edfe71c5
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g' a549befaa3733bba26d3bd06aabb222aba383728
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h' 835b80bc51f8f5c098dfc4fdccebde9a61f606ba
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i' f2a55fea1b818b3ddf7b623f3854c445b802552d
  1 trees fetched over *s (glob)
  fetching tree 'a/b/c/d/e/f/g/h/i/j' 798352a5c06a9995fe8ab9d657963810a6e5e603
  1 trees fetched over *s (glob)
  12 files fetched over 1 fetches - (12 misses, 0.00% hit ratio) over 0.00s (?)

Try again, disabling the descendantrevfastpath
  $ clearcache
  $ echo more >> a/b/c/d/e/f/g/h/i/j/file3
  $ hg amend -A --config remotefilelog.debug=True --config remotefilelog.descendantrevfastpath=False
  fetching tree '' efa8fa4352b919302f90e85924e691a632d6bea0, found via 9f95b8f1011f
  11 trees fetched over *s (glob)
