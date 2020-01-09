  $ . "$TESTDIR/library.sh"
  $ setconfig devel.print-metrics=1


  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm 'add x'
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master client -q
  fetching tree '' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  { metrics : { ssh : { connections : 2,
                        getpack : { calls : 1,  revs : 1},
                        gettreepack : { basemfnodes : 0,
                                        calls : 1,
                                        mfnodes : 1},
                        read : { bytes : 1982},
                        write : { bytes : 976}}}}
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > autocreatetrees=True
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
                        read : { bytes : 1125},
                        write : { bytes : 667}}}}
  $ hg manifest -r tip
  fetching tree '' 13532a59874531175abc845749c0491b36effb18, based on bc0c2c938b929f98b1c31a8c5994396ebb096bf0, found via 92f4ca0e667c
  1 trees fetched over 0.00s
  x
  z
  { metrics : { ssh : { connections : 1,
                        gettreepack : { basemfnodes : 1,
                                        calls : 1,
                                        mfnodes : 1},
                        read : { bytes : 872},
                        write : { bytes : 263}}}}
  $ hg debughistorypack $TESTTMP/hgcache/master/packs/manifests/*.histidx
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  13532a598745  bc0c2c938b92  ddb35f099a64  92f4ca0e667c  
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  bc0c2c938b92  000000000000  000000000000  085784c01c08  
