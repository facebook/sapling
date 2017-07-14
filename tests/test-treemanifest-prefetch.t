  $ CACHEDIR=`pwd`/hgcache
  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ . "$TESTDIR/library.sh"

  $ hg init master
  $ cd master
  $ mkdir dir
  $ echo x > dir/x
  $ hg commit -qAm 'add x'
  $ mkdir subdir
  $ echo z > subdir/z
  $ hg commit -qAm 'add subdir/z'
  $ echo x >> dir/x
  $ hg commit -Am 'modify x'
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../treemanifest
  > 
  > [remotefilelog]
  > name=master
  > cachepath=$CACHEDIR
  > usefastdatapack=True
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > 
  > [treemanifest]
  > server=True
  > EOF

  $ cd ..
  $ hg clone ssh://user@dummy/master client
  streaming all changes
  4 files to transfer, 952 bytes of data
  transferred 952 bytes in * seconds (*) (glob)
  searching for changes
  no changes found
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd master
  $ hg backfilltree

  $ cd ../client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../treemanifest
  > fastmanifest=$TESTDIR/../fastmanifest
  > [remotefilelog]
  > reponame = master
  > cachepath = $CACHEDIR
  > [fastmanifest]
  > usetree = True
  > usecache = False
  > EOF

Test prefetchtrees
  $ hg prefetchtrees -r '0 + 1 + 2'
  6 trees fetched over * (glob)
  $ ls $CACHEDIR/master/packs/manifests
  29938257d506f677320d5abec8e34a1a9ed635fe.histidx
  29938257d506f677320d5abec8e34a1a9ed635fe.histpack
  8adc618d23082c0a5311a4bbf9ac08b9b9672471.dataidx
  8adc618d23082c0a5311a4bbf9ac08b9b9672471.datapack
  $ hg debugdatapack --long $CACHEDIR/master/packs/manifests/*.dataidx
  
  subdir
  Node                                      Delta Base                                Delta Length
  ddb35f099a648a43a997aef53123bce309c794fd  0000000000000000000000000000000000000000  43
  
  
  Node                                      Delta Base                                Delta Length
  1be4ab2126dd2252dcae6be2aac2561dd3ddcda0  0000000000000000000000000000000000000000  95
  
  dir
  Node                                      Delta Base                                Delta Length
  a18d21674e76d6aab2edb46810b20fbdbd10fb4b  0000000000000000000000000000000000000000  43
  
  
  Node                                      Delta Base                                Delta Length
  60a7f7acb6bb5aaf93ca7d9062931b0f6a0d6db5  0000000000000000000000000000000000000000  95
  
  dir
  Node                                      Delta Base                                Delta Length
  bc0c2c938b929f98b1c31a8c5994396ebb096bf0  0000000000000000000000000000000000000000  43
  
  
  Node                                      Delta Base                                Delta Length
  ef362f8bbe8aa457b0cfc49f200cbeb7747984ed  0000000000000000000000000000000000000000  46
  $ hg debughistorypack $CACHEDIR/master/packs/manifests/*.histidx
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  60a7f7acb6bb  1be4ab2126dd  000000000000  bd6f9b289c01  
  1be4ab2126dd  ef362f8bbe8a  000000000000  f15c65c6e9bd  
  ef362f8bbe8a  000000000000  000000000000  ecfb693caff5  
  
  dir
  Node          P1 Node       P2 Node       Link Node     Copy From
  a18d21674e76  bc0c2c938b92  000000000000  bd6f9b289c01  
  bc0c2c938b92  000000000000  000000000000  ecfb693caff5  
  
  subdir
  Node          P1 Node       P2 Node       Link Node     Copy From
  ddb35f099a64  000000000000  000000000000  f15c65c6e9bd  
  $ hg debugdatapack --node ef362f8bbe8aa457b0cfc49f200cbeb7747984ed $CACHEDIR/master/packs/manifests/*.dataidx
  
  
  Node                                      Delta Base                                Delta SHA1                                Delta Length
  ef362f8bbe8aa457b0cfc49f200cbeb7747984ed  0000000000000000000000000000000000000000  3b295111780498d177793f9228bf736b915f0255  46
  $ hg -R ../master debugindex ../master/.hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      47     -1       0 ef362f8bbe8a 000000000000 000000000000
       1        47      61      0       1 1be4ab2126dd ef362f8bbe8a 000000000000
       2       108      58      1       2 60a7f7acb6bb 1be4ab2126dd 000000000000
  $ hg -R ../master debugindex ../master/.hg/store/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      48     -1       0 ef362f8bbe8a 000000000000 000000000000
       1        48      62      0       1 1be4ab2126dd ef362f8bbe8a 000000000000
       2       110      59      1       2 60a7f7acb6bb 1be4ab2126dd 000000000000

Test prefetch with base node (subdir/ shouldn't show up in the pack)
  $ rm -rf $CACHEDIR/master
  $ hg prefetchtrees -r '2' --base '1'
  2 trees fetched over * (glob)
  $ ls $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/3fb59713808147bda39cbd97b9cd862406f5865c.dataidx

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/3fb59713808147bda39cbd97b9cd862406f5865c.dataidx
  
  dir
  Node          Delta Base    Delta Length
  a18d21674e76  000000000000  43
  
  
  Node          Delta Base    Delta Length
  60a7f7acb6bb  000000000000  95

Test auto prefetch during normal access
  $ rm -rf $CACHEDIR/master
|| ( exit 1 ) is needed because ls on OSX and Linux exits differently
  $ ls $CACHEDIR/master/packs/manifests || ( exit 1 )
  *No such file or directory (glob)
  [1]
  $ hg log -r tip --stat --pager=off
  3 trees fetched over * (glob)
  2 trees fetched over * (glob)
  changeset:   2:bd6f9b289c01
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify x
  
   dir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ ls $CACHEDIR/master/packs/manifests
  148e9eb32f473ea522c591c95be0f9e772be9675.dataidx
  148e9eb32f473ea522c591c95be0f9e772be9675.datapack
  3fb59713808147bda39cbd97b9cd862406f5865c.dataidx
  3fb59713808147bda39cbd97b9cd862406f5865c.datapack
  524ab81400c6bc8449e3e720d81f836ebacec539.histidx
  524ab81400c6bc8449e3e720d81f836ebacec539.histpack
  e5c44a5c1bbfd8841df1c6c4b7cca54536e016db.histidx
  e5c44a5c1bbfd8841df1c6c4b7cca54536e016db.histpack

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/148e9eb32f473ea522c591c95be0f9e772be9675
  
  dir
  Node          Delta Base    Delta Length
  bc0c2c938b92  000000000000  43
  
  subdir
  Node          Delta Base    Delta Length
  ddb35f099a64  000000000000  43
  
  
  Node          Delta Base    Delta Length
  1be4ab2126dd  000000000000  95

- Note that subdir/ is not downloaded again
  $ hg debugdatapack $CACHEDIR/master/packs/manifests/3fb59713808147bda39cbd97b9cd862406f5865c
  
  dir
  Node          Delta Base    Delta Length
  a18d21674e76  000000000000  43
  
  
  Node          Delta Base    Delta Length
  60a7f7acb6bb  000000000000  95

Test that auto prefetch scans up the changelog for base trees
  $ rm -rf $CACHEDIR/master
  $ hg prefetchtrees -r 'tip^'
  3 trees fetched over * (glob)
  $ rm -rf $CACHEDIR/master
  $ hg prefetchtrees -r tip
  3 trees fetched over * (glob)
- Only 2 of the 3 trees from tip^ are downloaded as part of --stat's fetch
  $ hg log -r tip --stat --pager=off > /dev/null
  2 trees fetched over * (glob)

Test auto prefetch during pull

- Prefetch everything
  $ echo a >> a
  $ hg commit -Aqm 'draft commit that shouldnt affect prefetch'
  $ rm -rf $CACHEDIR/master
  $ hg pull --config treemanifest.pullprefetchcount=10 --traceback
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching trees
  6 trees fetched over * (glob)
  $ hg debugdatapack $CACHEDIR/master/packs/manifests/*.dataidx
  
  subdir
  Node          Delta Base    Delta Length
  ddb35f099a64  000000000000  43
  
  
  Node          Delta Base    Delta Length
  1be4ab2126dd  000000000000  95
  
  dir
  Node          Delta Base    Delta Length
  a18d21674e76  000000000000  43
  
  
  Node          Delta Base    Delta Length
  60a7f7acb6bb  000000000000  95
  
  dir
  Node          Delta Base    Delta Length
  bc0c2c938b92  000000000000  43
  
  
  Node          Delta Base    Delta Length
  ef362f8bbe8a  000000000000  46

  $ hg strip -q -r 'draft()'

- Prefetch just the top manifest (but the full one)
  $ rm -rf $CACHEDIR/master
  $ hg pull --config treemanifest.pullprefetchcount=1 --traceback
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching trees
  3 trees fetched over * (glob)
  $ hg debugdatapack $CACHEDIR/master/packs/manifests/*.dataidx
  
  dir
  Node          Delta Base    Delta Length
  a18d21674e76  000000000000  43
  
  subdir
  Node          Delta Base    Delta Length
  ddb35f099a64  000000000000  43
  
  
  Node          Delta Base    Delta Length
  60a7f7acb6bb  000000000000  95

- Prefetch commit 1 then minimally prefetch commit 2
  $ rm -rf $CACHEDIR/master
  $ hg prefetchtrees -r 1
  3 trees fetched over * (glob)
  $ ls $CACHEDIR/master/packs/manifests/*dataidx
  $TESTTMP/hgcache/master/packs/manifests/148e9eb32f473ea522c591c95be0f9e772be9675.dataidx
  $ hg pull --config treemanifest.pullprefetchcount=1 --traceback
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching trees
  2 trees fetched over * (glob)
  $ ls $CACHEDIR/master/packs/manifests/*dataidx
  $TESTTMP/hgcache/master/packs/manifests/148e9eb32f473ea522c591c95be0f9e772be9675.dataidx
  $TESTTMP/hgcache/master/packs/manifests/3fb59713808147bda39cbd97b9cd862406f5865c.dataidx
  $ hg debugdatapack $CACHEDIR/master/packs/manifests/3fb59713808147bda39cbd97b9cd862406f5865c.dataidx
  
  dir
  Node          Delta Base    Delta Length
  a18d21674e76  000000000000  43
  
  
  Node          Delta Base    Delta Length
  60a7f7acb6bb  000000000000  95

Test prefetching certain revs during pull
  $ cd ../master
  $ echo x >> dir/x
  $ hg commit -qm "modify dir/x a third time"
  $ echo x >> dir/x
  $ hg commit -qm "modify dir/x a fourth time"

  $ cd ../client
  $ rm -rf $CACHEDIR/master
  $ hg pull --config treemanifest.pullprefetchrevs='tip~2'
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  (run 'hg update' to get a working copy)
  prefetching trees
  3 trees fetched over * (glob)
  $ hg debugdatapack $CACHEDIR/master/packs/manifests/*.dataidx
  
  dir
  Node          Delta Base    Delta Length
  a18d21674e76  000000000000  43
  
  subdir
  Node          Delta Base    Delta Length
  ddb35f099a64  000000000000  43
  
  
  Node          Delta Base    Delta Length
  60a7f7acb6bb  000000000000  95

- Test prefetching only the new tree parts for a commit who's parent tree is not
- downloaded already. Note that subdir/z was not downloaded this time.
  $ hg pull --config treemanifest.pullprefetchrevs='tip'
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching trees
  2 trees fetched over * (glob)
  $ hg debugdatapack $CACHEDIR/master/packs/manifests/99050e724a9236121684523ba3f4db270e62fb58.dataidx
  
  dir
  Node          Delta Base    Delta Length
  bf22bc15398b  000000000000  43
  
  
  Node          Delta Base    Delta Length
  aa52a49be522  000000000000  95

Test that prefetch refills just part of a tree when the cache is deleted

  $ echo >> dir/x
  $ hg commit -m 'edit x locally'
  created new head
  $ rm -rf $CACHEDIR/master/*
  $ hg cat subdir/z
  3 trees fetched over * (glob)
  z

Test prefetch non-parent commits with no base node (should fetch minimal
trees - in this case 3 trees for commit 2, and 2 for commit 4 despite it having
3 directories)
  $ rm -rf $CACHEDIR/master
  $ hg prefetchtrees -r '2 + 4'
  5 trees fetched over * (glob)
