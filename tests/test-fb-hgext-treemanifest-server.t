  $ setconfig extensions.treemanifest=!
#testcases vfscachestore simplecachestore
  $ setconfig treemanifest.flatcompat=False treemanifest.treeonly=False

TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
  $ . "$TESTDIR/library.sh"


- Disable simplecache since it can cause certain reads to not actually hit the
- ondisk structures.
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > simplecache=!
  > EOF

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > treemanifest=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > EOF

#if simplecachestore
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > simplecacheserverstore=True
  > cacheserverstore=False
  > [extensions]
  > simplecache=
  > [simplecache]
  > cachedir=$TESTTMP/master/.hg/hgsimplecache
  > caches=local
  > EOF
#endif

Test that local commits on the server produce trees
  $ mkdir subdir
  $ echo x > subdir/x
  $ hg commit -qAm 'add subdir/x'

Create client
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master client -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > fastmanifest=
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > [treemanifest]
  > demanddownload=True
  > sendtrees=True
  > EOF

Test committing auto-downloads server trees and produces local trees
  $ [ -d $CACHEDIR/master/packs/manifests/ ]
  [1]
  $ [ -d .hg/store/packs/manifests/ ]
  [1]

  $ mkdir subdir2
  $ echo z >> subdir2/z
  $ hg commit -qAm "add subdir2/z"
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/878a145025fb3997b91efd9bb5f384e27d81f327:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  bc0c2c938b92  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  85b359fdb09e  000000000000  49            (missing)
  

  $ hg debugdatapack .hg/store/packs/manifests/*.dataidx
  .hg/store/packs/manifests/142949b3e7a62ab1d76f6d0720ca3117b819da1f:
  subdir2:
  Node          Delta Base    Delta Length  Blob Size
  ddb35f099a64  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  54cbf534b62b  000000000000  99            (missing)
  

Test pushing only flat manifests without pushrebase creates trees
  $ hg push --config treemanifest.sendtrees=False
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ hg --cwd ../master debugindex .hg/store/meta/subdir2/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       1 ddb35f099a64 000000000000 000000000000
  $ hg debugdatapack .hg/store/packs/manifests/*.datapack
  .hg/store/packs/manifests/142949b3e7a62ab1d76f6d0720ca3117b819da1f:
  subdir2:
  Node          Delta Base    Delta Length  Blob Size
  ddb35f099a64  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  54cbf534b62b  000000000000  99            (missing)
  
  $ hg --cwd ../master debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      51     -1       0 85b359fdb09e 000000000000 000000000000
       1        51      63      0       1 54cbf534b62b 85b359fdb09e 000000000000
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      51     -1       0 85b359fdb09e 000000000000 000000000000
       1        51      63      0       1 54cbf534b62b 85b359fdb09e 000000000000
  $ hg --cwd ../master debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      50     -1       0 85b359fdb09e 000000000000 000000000000
       1        50      62      0       1 54cbf534b62b 85b359fdb09e 000000000000
  $ hg -R ../master debugstrip -r tip
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/15486e46ccf6-fc9a70e1-backup.hg
  $ hg phase -dfr .

Test pushing only flat fails if forcetreereceive is on
  $ cat >> ../master/.hg/hgrc <<EOF
  > [pushrebase]
  > forcetreereceive=True
  > EOF
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ hg push --to mybook --config treemanifest.sendtrees=False
  pushing to ssh://user@dummy/master
  searching for changes
  remote: error: pushes must contain tree manifests when the server has pushrebase.forcetreereceive enabled
  abort: push failed on remote
  [255]

Test pushing flat and tree
  $ cat >> $TESTTMP/myhook.sh <<EOF
  > set -xe
  > [[ \$(hg log -r \$HG_NODE -T '{file_adds}') == 'subdir2/z' ]] && exit 1
  > exit 2
  > EOF
  $ cat >> $TESTTMP/myhook.py <<EOF
  > from edenscm.mercurial import bundlerepo
  > def myhook(ui=None, repo=None, hooktype=None, **hookargs):
  >     node = hookargs.get('node', None)
  >     ctx = repo[node]
  >     # Test comparing a flat and tree manifest
  >     ctx.p1().manifest().diff(ctx.manifest())
  > EOF
  $ chmod a+x $TESTTMP/myhook.sh
  $ cp ../master/.hg/hgrc ../master/.hg/hgrc.bak
  $ cat >> ../master/.hg/hgrc <<EOF
  > [hooks]
  > prepushrebase.myhookpy=python:$TESTTMP/myhook.py:myhook
  > prepushrebase.myhook=$TESTTMP/myhook.sh
  > EOF
  $ hg push --to mybook
  pushing to ssh://user@dummy/master
  searching for changes
  remote: +++ hg log -r 15486e46ccf6947fbb0a0209e6ce479e7f87ffae -T '{file_adds}'
  remote: ++ [[ subdir2/z == \s\u\b\d\i\r\2\/\z ]]
  remote: ++ exit 1
  remote: prepushrebase.myhook hook exited with status 1
  abort: push failed on remote
  [255]

Test pushing tree-only commit with commit hooks
  $ hg up -q '.^'
  $ mkdir subdir2
  $ echo >> subdir2/z
  $ hg commit -qAm 'add subdir2/z (treeonly)' --config treemanifest.treeonly=True
  $ hg push --to mybook -r .
  pushing to ssh://user@dummy/master
  searching for changes
  remote: +++ hg log -r aa8c79ec65bb33cc0dff01df2d70f8635cffc02d -T '{file_adds}'
  remote: ++ [[ subdir2/z == \s\u\b\d\i\r\2\/\z ]]
  remote: ++ exit 1
  remote: prepushrebase.myhook hook exited with status 1
  abort: push failed on remote
  [255]
  $ mv ../master/.hg/hgrc.bak ../master/.hg/hgrc

Test pushing only trees (no flats) with pushrebase creates trees on the server
  $ hg push --to mybook -r .
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 1 changeset:
  remote:     aa8c79ec65bb  add subdir2/z (treeonly)
  remote: 1 new changeset from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  $ ls ../master/.hg/store/meta
  subdir
  subdir2
- Verify it doesn't put anything in the pack directory
  $ ls_l ../master/.hg/store | grep pack
  [1]
  $ cd ../master

Verify flat was updated and tree was updated, even though only tree was sent
  $ hg debugdata .hg/store/00manifest.i 1
  subdir/x\x001406e74118627694268417491f018a4a883152f0 (esc)
  subdir2/z\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)

  $ hg debugdata .hg/store/00manifesttree.i 1
  subdir\x00bc0c2c938b929f98b1c31a8c5994396ebb096bf0t (esc)
  subdir2\x0002fd4859c40acf72a0ce0f75c2f8bef76935f3dct (esc)

  $ hg debugdata .hg/store/meta/subdir2/00manifest.i 0
  z\x00cc31c19aff7dbbbed214ec304839a8003fdd0b10 (esc)

Test stripping trees
  $ hg up -q tip
  $ echo a >> subdir/a
  $ hg commit -Aqm 'modify subdir/a'
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      50     -1       0 85b359fdb09e 000000000000 000000000000
       1        50      62      0       1 7e680cec965b 85b359fdb09e 000000000000
       2       112      61      1       2 d03189a14084 7e680cec965b 000000000000
  $ hg debugindex .hg/store/meta/subdir/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      54      0       2 126c4ddee02e bc0c2c938b92 000000000000
  $ hg debugstrip -r tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/4fd4fee9fca1-46b625db-backup.hg (glob)
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      50     -1       0 85b359fdb09e 000000000000 000000000000
       1        50      62      0       1 7e680cec965b 85b359fdb09e 000000000000
  $ hg debugindex .hg/store/meta/subdir/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000

Test stripping merge commits where filelogs arent affected
  $ rsync -a .hg/ $TESTTMP/backup.hg/
  $ echo a >> subdir/a
  $ hg commit -Aqm one
  $ hg up -q '.^'
  $ echo b >> subdir/b
  $ hg commit -Aqm two
  $ hg merge -q 'first(children(.^))'
  $ hg commit -m 'merge'
  $ hg log -r . -T '{rev}\n'
  4
  $ hg debugindex .hg/store/meta/subdir/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      54      0       2 126c4ddee02e bc0c2c938b92 000000000000
       2        98      54      0       3 abeda9251d1d bc0c2c938b92 000000000000
       3       152      54      2       4 d1018f351d1e abeda9251d1d 126c4ddee02e
- Verify rev 3 (from the merge commit) is gone after the strip
  $ hg debugstrip -r tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/a03b8b42d703-fdc98185-backup.hg
  $ hg debugindex .hg/store/meta/subdir/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      54      0       2 126c4ddee02e bc0c2c938b92 000000000000
       2        98      54      0       3 abeda9251d1d bc0c2c938b92 000000000000
  $ hg debugstrip -qr 'children(.^)'
  $ rm -rf .hg
  $ cp -R $TESTTMP/backup.hg .hg
  $ rm -rf $TESTTMP/backup.hg

Test pushing only trees without pushrebase to a hybrid server
  $ cd ../client
  $ hg push -f -r . --config extensions.pushrebase=!
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 0 changes to 0 files (+1 heads)
  remote: transaction abort!
  remote: rollback completed
  remote: cannot push only trees to a hybrid server without pushrebase
  abort: push failed on remote
  [255]

Test fetching from the server populates the cache
  $ cd ../
  $ hgcloneshallow ssh://user@dummy/master client2 -q -U
  $ cd client2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > fastmanifest=
  > pushrebase=
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > [treemanifest]
  > demanddownload=True
  > sendtrees=True
  > treeonly=True
  > EOF
  $ rm -rf .hg/store/00manifest*
  $ clearcache
  $ hg status --change tip > /dev/null
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  fetching tree '' 7e680cec965bd202ea244b3c4869181424ca5fe8, based on 85b359fdb09e9b8d7ac4a74551612b277345e8fd, found via a30b520ebf7a
  2 trees fetched over * (glob)
#if simplecachestore
  $ find ../master/.hg/hgsimplecache/trees/v2/get -type f | wc -l
  \s*4 (re)
  $ find ../master/.hg/hgsimplecache/trees/v2/nodeinfo -type f | wc -l
  \s*4 (re)
#else
  $ find ../master/.hg/cache/trees/v2/get -type f | wc -l
  \s*4 (re)
  $ find ../master/.hg/cache/trees/v2/nodeinfo -type f | wc -l
  \s*4 (re)
#endif

- Move the revlogs away to show that the cache is answering prefetches
  $ mv ../master/.hg/store/meta ../master/.hg/store/meta.bak
  $ clearcache
  $ hg status --change tip > /dev/null
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  fetching tree '' 7e680cec965bd202ea244b3c4869181424ca5fe8, based on 85b359fdb09e9b8d7ac4a74551612b277345e8fd, found via a30b520ebf7a
  2 trees fetched over * (glob)

- Corrupt the cache with the wrong value for a key and verify it notices
- (by going past the cache and failing to access the revlog)
#if simplecachestore
  $ cp ../master/.hg/hgsimplecache/trees/v2/get/0b/0fa4abc415aa6a46e003c61283b182ccc989b6:v2 ../master/.hg/hgsimplecache/trees/v2/get/d4/395b5ffa18499864439ac2b1a731ff7b7491fa:v2
#else
  $ cp ../master/.hg/cache/trees/v2/get/0b/0fa4abc415aa6a46e003c61283b182ccc989b6 ../master/.hg/cache/trees/v2/get/d4/395b5ffa18499864439ac2b1a731ff7b7491fa
#endif
  $ clearcache
The server sometimes throws spurious errors, see: D14446457
  $ hg status --change tip 2>&1 > /dev/null | grep -v '^remote:'
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  fetching tree '' 7e680cec965bd202ea244b3c4869181424ca5fe8, based on 85b359fdb09e9b8d7ac4a74551612b277345e8fd, found via a30b520ebf7a
  abort: "unable to find the following nodes locally or on the server: ('', 7e680cec965bd202ea244b3c4869181424ca5fe8)"

- Verify the cache remediates itself from the corruption
- (now that the revlogs are back)
  $ clearcache
  $ mv ../master/.hg/store/meta.bak ../master/.hg/store/meta
  $ hg status --change tip > /dev/null
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  fetching tree '' 7e680cec965bd202ea244b3c4869181424ca5fe8, based on 85b359fdb09e9b8d7ac4a74551612b277345e8fd, found via a30b520ebf7a
  2 trees fetched over * (glob)

- Ensure the server evicts the cache
  $ cat >> ../master/.hg/hgrc <<EOF
  > [treemanifest]
  > servermaxcachesize=0
  > servercacheevictionpercent=90
  > EOF
#if simplecachestore
  $ find ../master/.hg/hgsimplecache/trees/v2/nodeinfo -type f | xargs -n 1 -I{} cp {} {}2
  $ find ../master/.hg/hgsimplecache/trees/v2/nodeinfo -type f | xargs -n 1 -I{} cp {} {}3
  $ find ../master/.hg/hgsimplecache/trees/v2/nodeinfo -type f | xargs -n 1 -I{} mv {} {}4
  $ find ../master/.hg/hgsimplecache/trees/v2/nodeinfo -type f | wc -l
  \s*16 (re)
#else
  $ find ../master/.hg/cache/trees/v2/nodeinfo -type f | xargs -n 1 -I{} cp {} {}2
  $ find ../master/.hg/cache/trees/v2/nodeinfo -type f | xargs -n 1 -I{} cp {} {}3
  $ find ../master/.hg/cache/trees/v2/nodeinfo -type f | xargs -n 1 -I{} mv {} {}4
  $ find ../master/.hg/cache/trees/v2/nodeinfo -type f | wc -l
  \s*16 (re)
#endif
  $ clearcache
  $ hg status --change tip
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  fetching tree '' 7e680cec965bd202ea244b3c4869181424ca5fe8, based on 85b359fdb09e9b8d7ac4a74551612b277345e8fd, found via a30b520ebf7a
  2 trees fetched over * (glob)
  A subdir2/z
simplecachestore doesn't have eviction policy
#if simplecachestore
  $ find ../master/.hg/hgsimplecache/trees/v2/nodeinfo -type f | wc -l
  \s*20 (re)
#else
  $ find ../master/.hg/cache/trees/v2/nodeinfo -type f | wc -l
  \s*8 (re)
#endif

Try pulling while treemanifest.blocksendflat is True
  $ cat >> ../master/.hg/hgrc <<EOF
  > [treemanifest]
  > blocksendflat=True
  > EOF

- Pull to a treeonly repo
  $ hg config treemanifest.treeonly
  True
  $ hg debugstrip -qr a30b520ebf7a
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets a30b520ebf7a
  $ hg status --change a30b520ebf7a
  A subdir2/z

- Pull to a flat manifest only repo
  $ cd ../client
  $ hg config treemanifest.treeonly
  False
  $ hg debugstrip -qr a30b520ebf7a
  $ hg pull --config extension.treemanifest=! --config fastmanifest.usetree=False 1>/dev/null
  remote: abort: must produce treeonly changegroups in a treeonly repository
  transaction abort!
  rollback completed
  abort: pull failed on remote
  [255]

- Pull to a hybrid manifest repo
  $ hg pull 1>/dev/null
  remote: abort: must produce treeonly changegroups in a treeonly repository
  transaction abort!
  rollback completed
  abort: pull failed on remote
  [255]

- Bypass the block
  $ hg pull --config treemanifest.forceallowflat=True
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  new changesets a30b520ebf7a

Attempt to push from a treeonly repo without sending trees
  $ cd ../client2
  $ hg up tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)
  $ echo >> subdir2/z
  $ hg commit -qm "Edit subdir2/z"
  $ hg push --config treemanifest.sendtrees=False
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: error: pretxnclose.checkmanifest hook failed: attempting to close transaction which includes commits (ab5f5b4a91cff8dbded4c96f5b2c3e7d0995c882) without manifests (9921ee5733f3898386a12357ec28a58057fe32d9)
  remote: transaction abort!
  remote: rollback completed
  remote: attempting to close transaction which includes commits (ab5f5b4a91cff8dbded4c96f5b2c3e7d0995c882) without manifests (9921ee5733f3898386a12357ec28a58057fe32d9)
  abort: push failed on remote
  [255]

  $ hg -R ../master export tip > /dev/null

Stripping in a treeonly server
  $ cat >> ../master/.hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > EOF
  $ hg push --to master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 1 changeset:
  remote:     ab5f5b4a91cf  Edit subdir2/z

  $ cd ../master
  $ ls -l .hg/store/meta/subdir2/00manifest.i
  * 216 * .hg/store/meta/subdir2/00manifest.i (glob)
  $ ls -l .hg/store/00manifesttree.i
  * 366 * .hg/store/00manifesttree.i (glob)
  $ hg debugstrip -r tip --config treemanifest.blocksendflat=False
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/ab5f5b4a91cf-cb006139-backup.hg
  $ ls -l .hg/store/meta/subdir2/00manifest.i
  * 108 * .hg/store/meta/subdir2/00manifest.i (glob)
  $ ls -l .hg/store/00manifesttree.i
  * 240 * .hg/store/00manifesttree.i (glob)
