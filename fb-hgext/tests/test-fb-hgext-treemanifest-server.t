  $ . "$TESTDIR/library.sh"

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=$TESTDIR/../hgext3rd/pushrebase.py
  > treemanifest=$TESTDIR/../treemanifest
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > EOF

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
  > treemanifest=$TESTDIR/../treemanifest
  > fastmanifest=$TESTDIR/../fastmanifest
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
  2 trees fetched over * (glob)

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/*.dataidx
  $TESTTMP/hgcache/master/packs/manifests/4d21ecb6c95e12dcf807b793cd1c55eeed861734:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  bc0c2c938b92  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  85b359fdb09e  000000000000  49            (missing)
  

  $ hg debugdatapack .hg/store/packs/manifests/*.dataidx
  .hg/store/packs/manifests/e3876af326e0e51d1f3ea0444d2b1a7db2915763:
  subdir2:
  Node          Delta Base    Delta Length  Blob Size
  ddb35f099a64  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  54cbf534b62b  000000000000  99            (missing)
  

Test pushing without pushrebase fails
  $ hg push
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: transaction abort!
  remote: rollback completed
  remote: cannot push commits to a treemanifest transition server without pushrebase
  abort: push failed on remote
  [255]

Test pushing only flat fails if forcetreereceive is on
  $ cat >> ../master/.hg/hgrc <<EOF
  > [pushrebase]
  > forcetreereceive=True
  > EOF
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=$TESTDIR/../hgext3rd/pushrebase.py
  > EOF
  $ mv .hg/store/packs .hg/store/packs.bak
  $ hg push --to mybook
  pushing to ssh://user@dummy/master
  searching for changes
  remote: error: pushes must contain tree manifests when the server has pushrebase.forcetreereceive enabled
  abort: push failed on remote
  [255]
  $ mv .hg/store/packs.bak .hg/store/packs

Test pushing flat and tree
  $ cat >> $TESTTMP/myhook.sh <<EOF
  > set -xe
  > [[ \$(hg log -r \$HG_NODE -T '{file_adds}') == 'subdir2/z' ]] && exit 1
  > exit 2
  > EOF
  $ chmod a+x $TESTTMP/myhook.sh
  $ cp ../master/.hg/hgrc ../master/.hg/hgrc.bak
  $ cat >> ../master/.hg/hgrc <<EOF
  > [hooks]
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
  1 trees fetched over * (glob)
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
  $ hg strip -r tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/4fd4fee9fca1-46b625db-backup.hg (glob)
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      50     -1       0 85b359fdb09e 000000000000 000000000000
       1        50      62      0       1 7e680cec965b 85b359fdb09e 000000000000
  $ hg debugindex .hg/store/meta/subdir/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
