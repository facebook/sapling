  $ . "$TESTDIR/library.sh"
  $ setconfig experimental.allowfilepeer=True
  $ setconfig devel.print-metrics=1 devel.skip-metrics=watchman
  $ setconfig remotefilelog.write-hgcache-to-indexedlog=False remotefilelog.write-local-to-indexedlog=False


  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
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
                        read : { bytes : 2067},
                        write : { bytes : 882}}}}
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
  $ hg up -q 085784c01c08984ae3b6f4e4a6e553035d58380b
  $ hg merge -q -r 'max(desc(add))'
  $ hg commit -qAm 'merge'

  $ cd ../client
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 1097},
                        write : { bytes : 657}}}}
  $ hg manifest -r tip
  fetching tree '' 13532a59874531175abc845749c0491b36effb18
  1 trees fetched over 0.00s
  x
  z
  { metrics : { ssh : { connections : 1,
                        gettreepack : { basemfnodes : 0,
                                        calls : 1,
                                        mfnodes : 1},
                        read : { bytes : 898},
                        write : { bytes : 219}}}}
  $ hg debughistorypack $TESTTMP/hgcache/master/packs/manifests/*.histidx
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  13532a598745  bc0c2c938b92  ddb35f099a64  000000000000  
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  bc0c2c938b92  000000000000  000000000000  000000000000  
