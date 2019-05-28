  $ setconfig extensions.treemanifest=!
There are three cases which are of interest in this test:

 - client remotefilelog enabled and the client repo is a shallowrepo
 In this case, the expectation is that the prefetch command will be able to
 prefetch both trees and files.

 - client remotefilelog enabled and the client repo is not a shallowrepo
 In this case, the expectation is that the prefetch command will be able to only
 prefetch trees.

 - client remotefilelog disabled
 In this case, the expectation is that the prefetch command will be able to only
 prefetch trees and will not be able to perform operations like repack which
 require remotefilelog.

#testcases remotefilelog.true.shallowrepo.true remotefilelog.true.shallowrepo.false remotefilelog.false

  $ CACHEDIR=`pwd`/hgcache
  $ setconfig treemanifest.treeonly=False

  $ . "$TESTDIR/library.sh"

  $ hginit master
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
  > treemanifest=
  > 
  > [remotefilelog]
  > server=True
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

#if remotefilelog.true.shallowrepo.true
  $ hgcloneshallow ssh://user@dummy/master client
  streaming all changes
  2 files to transfer, 749 bytes of data
  transferred 749 bytes in * seconds (*) (glob)
  searching for changes
  no changes found
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)
#else
  $ hg clone ssh://user@dummy/master client
  streaming all changes
  4 files to transfer, 952 bytes of data
  transferred 952 bytes in * seconds (*) (glob)
  searching for changes
  no changes found
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

  $ cd master
  $ hg backfilltree

  $ cd ../client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > fastmanifest=
  > [remotefilelog]
  > reponame = master
  > prefetchdays=0
  > cachepath = $CACHEDIR
  > [fastmanifest]
  > usetree = True
  > usecache = False
  > EOF

#if remotefilelog.false
  $ cat >> .hg/hgrc <<EOF
  > 
  > [extensions]
  > remotefilelog=!
  > EOF
#endif

Test prefetch
  $ hg prefetch -r '0 + 1 + 2'
  6 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)
  $ ls $CACHEDIR/master/packs/manifests
  29938257d506f677320d5abec8e34a1a9ed635fe.histidx
  29938257d506f677320d5abec8e34a1a9ed635fe.histpack
  adc41dd93f904447812dd994d49bce59ea7c4360.dataidx
  adc41dd93f904447812dd994d49bce59ea7c4360.datapack
  $ hg debugdatapack --config extensions.remotefilelog= \
  > --long $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/adc41dd93f904447812dd994d49bce59ea7c4360:
  subdir:
  Node                                      Delta Base                                Delta Length  Blob Size
  ddb35f099a648a43a997aef53123bce309c794fd  0000000000000000000000000000000000000000  43            (missing)
  
  (empty name):
  Node                                      Delta Base                                Delta Length  Blob Size
  1be4ab2126dd2252dcae6be2aac2561dd3ddcda0  0000000000000000000000000000000000000000  95            (missing)
  
  dir:
  Node                                      Delta Base                                Delta Length  Blob Size
  a18d21674e76d6aab2edb46810b20fbdbd10fb4b  0000000000000000000000000000000000000000  43            (missing)
  
  (empty name):
  Node                                      Delta Base                                Delta Length  Blob Size
  60a7f7acb6bb5aaf93ca7d9062931b0f6a0d6db5  0000000000000000000000000000000000000000  95            (missing)
  
  dir:
  Node                                      Delta Base                                Delta Length  Blob Size
  bc0c2c938b929f98b1c31a8c5994396ebb096bf0  0000000000000000000000000000000000000000  43            (missing)
  
  (empty name):
  Node                                      Delta Base                                Delta Length  Blob Size
  ef362f8bbe8aa457b0cfc49f200cbeb7747984ed  0000000000000000000000000000000000000000  46            (missing)
  
  $ hg debughistorypack --config extensions.remotefilelog= \
  > $CACHEDIR/master/packs/manifests/*.histidx
  
  
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
  $ hg debugdatapack --config extensions.remotefilelog= \
  > --node-delta ef362f8bbe8aa457b0cfc49f200cbeb7747984ed $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/adc41dd93f904447812dd994d49bce59ea7c4360:
  
  
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

#if remotefilelog.true.shallowrepo.true
Multiple trees are fetched in this case because the file prefetching code path
requires tree manifest for the base commit.

  $ hg prefetch -r '2' --base '1'
  2 trees fetched over * (glob)
  fetching tree '' 1be4ab2126dd2252dcae6be2aac2561dd3ddcda0, based on 60a7f7acb6bb5aaf93ca7d9062931b0f6a0d6db5, found via f15c65c6e9bd
  2 trees fetched over * (glob)
  fetching tree 'subdir' ddb35f099a648a43a997aef53123bce309c794fd
  1 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ ls $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/9e2665e9046d365538eaa0f532dfd5c62aa1bf9c.dataidx

  $ hg debugdatapack $TESTTMP/hgcache/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/9e2665e9046d365538eaa0f532dfd5c62aa1bf9c:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  a18d21674e76  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  60a7f7acb6bb  000000000000  95            (missing)
  
  dir:
  Node          Delta Base    Delta Length  Blob Size
  bc0c2c938b92  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  1be4ab2126dd  000000000000  95            (missing)
  
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  ddb35f099a64  000000000000  43            (missing)
  
#else
  $ hg prefetch -r '2' --base '1'
  2 trees fetched over * (glob)
  $ ls $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/4113a1ecc22f9f280deb722133d462720d3d7a9d.dataidx

  $ hg debugdatapack --config extensions.remotefilelog= $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/4113a1ecc22f9f280deb722133d462720d3d7a9d:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  a18d21674e76  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  60a7f7acb6bb  000000000000  95            (missing)
  
#endif

Test prefetching when a draft commit is marked public
  $ mkdir $TESTTMP/cachedir.bak
  $ mv $CACHEDIR/* $TESTTMP/cachedir.bak

- Create a draft commit, and force it to be public
#if remotefilelog.true.shallowrepo.true
  $ hg prefetch -r .
  3 trees fetched over * (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)
#else
  $ hg prefetch -r .
  3 trees fetched over * (glob)
#endif
  $ echo foo > foo
  $ hg commit -Aqm 'add foo'
  $ hg phase -p -r .
  $ hg log -G -T '{rev} {phase} {manifest}'
  @  3 public 5cf0d3bd4f40594eff7f0c945bec8baa8d115d01
  |
  o  2 public 60a7f7acb6bb5aaf93ca7d9062931b0f6a0d6db5
  |
  o  1 public 1be4ab2126dd2252dcae6be2aac2561dd3ddcda0
  |
  o  0 public ef362f8bbe8aa457b0cfc49f200cbeb7747984ed
  
- Add remotenames for the remote heads
  $ hg pull --config extensions.remotenames=
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found

- Attempt to download the latest server commit. Verify there's no error about a
- missing manifest from the server.
  $ clearcache
  $ hg status --change 2 --config extensions.remotenames=
  fetching tree '' 1be4ab2126dd2252dcae6be2aac2561dd3ddcda0, found via f15c65c6e9bd
  3 trees fetched over * (glob)
  fetching tree '' 60a7f7acb6bb5aaf93ca7d9062931b0f6a0d6db5, based on 1be4ab2126dd2252dcae6be2aac2561dd3ddcda0, found via bd6f9b289c01
  2 trees fetched over * (glob)
  M dir/x
  $ hg debugstrip -r 3
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/b6308255e316-b2a7dcf7-backup.hg

  $ clearcache
  $ mv $TESTTMP/cachedir.bak/* $CACHEDIR

Test auto prefetch during normal access
  $ rm -rf $CACHEDIR/master
|| ( exit 1 ) is needed because ls on OSX and Linux exits differently
  $ ls $CACHEDIR/master/packs/manifests || ( exit 1 )
  * $ENOENT$ (glob)
  [1]
  $ hg log -r tip --stat --pager=off
  fetching tree '' 1be4ab2126dd2252dcae6be2aac2561dd3ddcda0, found via f15c65c6e9bd
  3 trees fetched over * (glob)
  fetching tree '' 60a7f7acb6bb5aaf93ca7d9062931b0f6a0d6db5, based on 1be4ab2126dd2252dcae6be2aac2561dd3ddcda0, found via bd6f9b289c01
  2 trees fetched over * (glob)
  changeset:   2:bd6f9b289c01
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify x
  
   dir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)
  $ ls $CACHEDIR/master/packs/manifests
  0ac61f8fd04a6623ecbde9812036089c557f07fa.histidx
  0ac61f8fd04a6623ecbde9812036089c557f07fa.histpack
  93d481677f2b09bd1cec155608977cb9806df077.dataidx
  93d481677f2b09bd1cec155608977cb9806df077.datapack

  $ hg debugdatapack --config extensions.remotefilelog= $CACHEDIR/master/packs/manifests/*.datapack
  $TESTTMP/hgcache/master/packs/manifests/93d481677f2b09bd1cec155608977cb9806df077:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  bc0c2c938b92  000000000000  43            (missing)
  
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  ddb35f099a64  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  1be4ab2126dd  000000000000  95            (missing)
  
  dir:
  Node          Delta Base    Delta Length  Blob Size
  a18d21674e76  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  60a7f7acb6bb  000000000000  95            (missing)
  
Test that auto prefetch scans up the changelog for base trees
  $ rm -rf $CACHEDIR/master
  $ hg prefetch -r 'tip^'
  3 trees fetched over * (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)
  $ rm -rf $CACHEDIR/master
  $ hg prefetch -r tip
  3 trees fetched over * (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)
- Only 2 of the 3 trees from tip^ are downloaded as part of --stat's fetch
  $ hg log -r tip --stat --pager=off > /dev/null
  fetching tree '' 1be4ab2126dd2252dcae6be2aac2561dd3ddcda0, based on 60a7f7acb6bb5aaf93ca7d9062931b0f6a0d6db5, found via f15c65c6e9bd
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)

Test auto prefetch during pull

- Prefetch everything
  $ echo a >> a
  $ hg commit -Aqm 'draft commit that shouldnt affect prefetch'
  $ rm -rf $CACHEDIR/master
  $ hg pull --config treemanifest.pullprefetchcount=10 --traceback
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching trees for 3 commits
  6 trees fetched over * (glob)
  $ hg debugdatapack --config extensions.remotefilelog= \
  > $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/adc41dd93f904447812dd994d49bce59ea7c4360:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  ddb35f099a64  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  1be4ab2126dd  000000000000  95            (missing)
  
  dir:
  Node          Delta Base    Delta Length  Blob Size
  a18d21674e76  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  60a7f7acb6bb  000000000000  95            (missing)
  
  dir:
  Node          Delta Base    Delta Length  Blob Size
  bc0c2c938b92  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  ef362f8bbe8a  000000000000  46            (missing)
  

  $ hg debugstrip -q -r 'draft()'

- Prefetch just the top manifest (but the full one)
  $ rm -rf $CACHEDIR/master
  $ hg pull --config treemanifest.pullprefetchcount=1 --traceback
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching tree for bd6f9b289c01
  3 trees fetched over * (glob)
  $ hg debugdatapack --config extensions.remotefilelog= \
  > $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/9c4f8bc12fc0d3a742847e75687c57cc0cafa334:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  a18d21674e76  000000000000  43            (missing)
  
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  ddb35f099a64  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  60a7f7acb6bb  000000000000  95            (missing)
  

- Prefetch commit 1 then minimally prefetch commit 2
  $ rm -rf $CACHEDIR/master
  $ hg prefetch -r 1
  3 trees fetched over * (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)
  $ ls $CACHEDIR/master/packs/manifests/*dataidx
  $TESTTMP/hgcache/master/packs/manifests/c3ae0c4afc5f96ac510fd7ea3dddd0720a6d4dfb.dataidx
  $ hg pull --config treemanifest.pullprefetchcount=1 --traceback
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching tree for bd6f9b289c01
  2 trees fetched over * (glob)
  $ ls $CACHEDIR/master/packs/manifests/*dataidx
  $TESTTMP/hgcache/master/packs/manifests/4113a1ecc22f9f280deb722133d462720d3d7a9d.dataidx
  $TESTTMP/hgcache/master/packs/manifests/c3ae0c4afc5f96ac510fd7ea3dddd0720a6d4dfb.dataidx
  $ hg debugdatapack --config extensions.remotefilelog= \
  >  $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/4113a1ecc22f9f280deb722133d462720d3d7a9d:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  a18d21674e76  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  60a7f7acb6bb  000000000000  95            (missing)
  
  $TESTTMP/hgcache/master/packs/manifests/c3ae0c4afc5f96ac510fd7ea3dddd0720a6d4dfb:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  bc0c2c938b92  000000000000  43            (missing)
  
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  ddb35f099a64  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  1be4ab2126dd  000000000000  95            (missing)
  

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
  added 2 changesets with 0 changes to 0 files (remotefilelog.true.shallowrepo.true !)
  added 2 changesets with 2 changes to 1 files (no-remotefilelog.true.shallowrepo.true !)
  new changesets dece825f8add:cfacdcc4cee5
  prefetching tree for bd6f9b289c01
  3 trees fetched over * (glob)
  $ hg debugdatapack --config extensions.remotefilelog= \
  > $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/9c4f8bc12fc0d3a742847e75687c57cc0cafa334:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  a18d21674e76  000000000000  43            (missing)
  
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  ddb35f099a64  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  60a7f7acb6bb  000000000000  95            (missing)
  

- Test prefetching only the new tree parts for a commit who's parent tree is not
- downloaded already. Note that subdir/z was not downloaded this time.
  $ hg pull --config treemanifest.pullprefetchrevs='tip'
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching tree for cfacdcc4cee5
  2 trees fetched over * (glob)
  $ hg debugdatapack --config extensions.remotefilelog= \
  > $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/59658c2bcbdcbfd3c836edc4cdca4f0297acc287:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  bf22bc15398b  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  aa52a49be522  000000000000  95            (missing)
  
  $TESTTMP/hgcache/master/packs/manifests/9c4f8bc12fc0d3a742847e75687c57cc0cafa334:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  a18d21674e76  000000000000  43            (missing)
  
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  ddb35f099a64  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  60a7f7acb6bb  000000000000  95            (missing)
  

Test that prefetch refills just part of a tree when the cache is deleted

  $ echo >> dir/x
  $ hg commit -m 'edit x locally'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)
  $ rm -rf $CACHEDIR/master/*
  $ hg cat subdir/z
  fetching tree 'subdir' ddb35f099a648a43a997aef53123bce309c794fd
  1 trees fetched over * (glob)
  z
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)

Test prefetch non-parent commits with no base node (should fetch minimal
trees - in this case 3 trees for commit 2, and 2 for commit 4 despite it having
3 directories)
  $ rm -rf $CACHEDIR/master
  $ hg prefetch -r '2 + 4'
  5 trees fetched over * (glob)
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)

Test repack option
  $ rm -rf $CACHEDIR/master

  $ hg prefetch -r '0'
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)
  $ hg prefetch -r '2'
  3 trees fetched over * (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)

#if remotefilelog.false
  $ hg prefetch -r '4' --repack
  abort: repack requires remotefilelog extension
  [255]
#else
  $ hg prefetch -r '4' --repack
  2 trees fetched over * (glob)
  (running background incremental repack)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (remotefilelog.true.shallowrepo.true !)

  $ sleep 0.5
  $ hg debugwaitonrepack
  $ ls_l $CACHEDIR/master/packs/manifests | grep datapack | wc -l
  \s*1 (re)
#endif

Test prefetching with no options works. The expectation is to prefetch the stuff
required for working with the draft commits which happens to be only revision 5
in this case.

  $ rm -rf $CACHEDIR/master

#if remotefilelog.true.shallowrepo.true
The tree prefetching code path fetches no trees for revision 5. However, the
file prefetching code path fetches 1 file for revision 5 and while doing so,
also fetches 3 trees dealing with the tree manifest of the base revision 2.

  $ hg prefetch
  fetching tree 'subdir' ddb35f099a648a43a997aef53123bce309c794fd
  1 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
#else
The tree prefetching code path fetches no trees for revision 5. And there is no
prefetching of files in this test case.
  $ hg prefetch
#endif

Running prefetch in the master repository should exit gracefully

  $ cd ../master
  $ hg prefetch
