  $ CACHEDIR=`pwd`/hgcache
  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ . "$TESTDIR/library.sh"

  $ hg init client1
  $ cd client1
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=
  > treemanifest=
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
  > fastmanifest=
  > treemanifest=
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
  36c63010eaf0a52d3536ced5e32bf4c6847a0e40.histidx
  36c63010eaf0a52d3536ced5e32bf4c6847a0e40.histpack
  fb1ee78215bfece34ca8e233fcf5e9fd69ec52bd.dataidx
  fb1ee78215bfece34ca8e233fcf5e9fd69ec52bd.datapack

Pushing p2p puts the received packs in the local pack store
  $ hg push -q ../client2
  $ ls ../client2/.hg/store/packs/manifests
  36c63010eaf0a52d3536ced5e32bf4c6847a0e40.histidx
  36c63010eaf0a52d3536ced5e32bf4c6847a0e40.histpack
  fb1ee78215bfece34ca8e233fcf5e9fd69ec52bd.dataidx
  fb1ee78215bfece34ca8e233fcf5e9fd69ec52bd.datapack
