  $ CACHEDIR=`pwd`/hgcache
  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ cat >> $HGRCPATH <<EOF
  > [treemanifest]
  > sendtrees=True
  > EOF

  $ . "$TESTDIR/library.sh"

  $ hg init client1
  $ cd client1
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=$TESTDIR/../fastmanifest
  > treemanifest=$TESTDIR/../treemanifest
  > 
  > [remotefilelog]
  > reponame=master
  > cachepath=$CACHEDIR
  > usefastdatapack=True
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > EOF

  $ echo a > a
  $ mkdir dir
  $ echo b > dir/b
  $ hg commit -Aqm 'initial commit'

  $ hg init ../client2
  $ cat >> ../client2/.hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=$TESTDIR/../fastmanifest
  > treemanifest=$TESTDIR/../treemanifest
  > 
  > [remotefilelog]
  > reponame=master
  > cachepath=$CACHEDIR
  > usefastdatapack=True
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > EOF

  $ ls .hg/store/packs/manifests
  15f45e8ca8affec27464278498594f94a3624815.histidx
  15f45e8ca8affec27464278498594f94a3624815.histpack
  fb1ee78215bfece34ca8e233fcf5e9fd69ec52bd.dataidx
  fb1ee78215bfece34ca8e233fcf5e9fd69ec52bd.datapack

Pushing with treemanifest disabled does nothing
(disable demand import so treemanifest.py is forced to load)
  $ HGDEMANDIMPORT=disable hg push -q ../client2 --config extensions.treemanifest=! --config fastmanifest.usetree=False
  $ ls ../client2/.hg/store/packs/manifests || true
  * No such file or directory (glob)

  $ hg -R ../client2 strip -q -r tip

Pushing p2p puts the received packs in the local pack store
  $ hg push -q ../client2
  $ ls ../client2/.hg/store/packs/manifests
  15f45e8ca8affec27464278498594f94a3624815.histidx
  15f45e8ca8affec27464278498594f94a3624815.histpack
  fb1ee78215bfece34ca8e233fcf5e9fd69ec52bd.dataidx
  fb1ee78215bfece34ca8e233fcf5e9fd69ec52bd.datapack
