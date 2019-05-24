  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > pushrebase=
  > remotenames=
  > [treemanifest]
  > sendtrees=True
  > EOF
  $ setconfig treemanifest.treeonly=False

Setup the server

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

Make local commits on the server
  $ mkdir subdir
  $ echo x > subdir/x
  $ hg commit -qAm 'add subdir/x'
  $ hg book master

The following will turn on sendtrees mode for a hybrid client and verify it
sends them during a push and during bundle operations.

Create flat manifest clients
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master client1 -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ hgcloneshallow ssh://user@dummy/master client2 -q

Transition to hybrid flat+tree client
  $ cat >> client1/.hg/hgrc <<EOF
  > [extensions]
  > amend=
  > fastmanifest=
  > treemanifest=
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > [treemanifest]
  > demanddownload=True
  > EOF
  $ cat >> client2/.hg/hgrc <<EOF
  > [extensions]
  > amend=
  > fastmanifest=
  > treemanifest=
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > [treemanifest]
  > demanddownload=True
  > EOF

Make a draft commit
  $ cd client1
  $ echo f >> subdir/x
  $ hg commit -qm "hybrid commit"
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  $ hg debugdatapack .hg/store/packs/manifests/*datapack
  .hg/store/packs/manifests/5395c3a9f408d2f2ffac93a2f1d6f039234be6ff:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  97c1b2747888  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  c0196aba344d  000000000000  49            (missing)
  

Test bundling/unbundling
  $ hg bundle -r . --base '.^' ../treebundle.hg --debug | grep treegroup
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload

  $ cd ../client2
  $ hg unbundle ../treebundle.hg --debug | grep treegroup
  bundle2-input-part: "b2x:treegroup2" (params: 3 mandatory) supported
  $ hg debugdatapack .hg/store/packs/manifests/*datapack
  .hg/store/packs/manifests/5395c3a9f408d2f2ffac93a2f1d6f039234be6ff:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  97c1b2747888  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  c0196aba344d  000000000000  49            (missing)
  
Test pushing
  $ hg push -r tip --to master --debug 2>&1 | grep rebasepackpart
  bundle2-output-part: "b2x:rebasepackpart" (params: 3 mandatory) streamed payload
