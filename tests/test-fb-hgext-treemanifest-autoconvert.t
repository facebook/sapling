  $ setconfig extensions.treemanifest=!
  $ . "$TESTDIR/library.sh"
  $ setconfig devel.print-metrics=1
  $ setconfig treemanifest.treeonly=False


  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm 'add x'
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master client -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  { metrics : { ssh : { connections : 2,
                        getpack : { calls : 1,  revs : 1},
                        read : { bytes : 1805},
                        write : { bytes : 803}}}}
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=
  > treemanifest=
  > 
  > [treemanifest]
  > autocreatetrees=True
  > demanddownload=False
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > EOF

Test auto creating trees for merge commit
  $ cd ../master
  $ hg up -q null
  $ echo z >> z
  $ hg commit -qAm 'add z'
  $ hg up -q 0
  $ hg merge -q -r 1
  $ hg commit -qAm 'merge'

  $ cd ../client
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  new changesets d32fd17cb041:92f4ca0e667c
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 1435},
                        write : { bytes : 655}}}}
  $ hg manifest -r tip
  x
  z
  $ hg debughistorypack $TESTTMP/hgcache/master/packs/manifests/*.histidx
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  ddb35f099a64  000000000000  000000000000  d32fd17cb041  
  13532a598745  bc0c2c938b92  000000000000  92f4ca0e667c  
  bc0c2c938b92  000000000000  000000000000  085784c01c08  
