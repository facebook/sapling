  $ setconfig extensions.treemanifest=!
  $ . "$TESTDIR/library.sh"
  $ setconfig devel.print-metrics=1
  $ setconfig treemanifest.treeonly=False

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > treemanifest=
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > EOF
  $ mkcommit a
  $ mkcommit b
  $ hg phase -p -r 'all()'

Clone it
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master client1 -q --config extensions.treemanifest= --config treemanifest.treeonly=True
  fetching tree '' a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7, found via d2ae7f538514
  1 trees fetched over * (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)
  { metrics : { ssh : { connections : 2,
                        getpack : { calls : 1,  revs : 2},
                        gettreepack : { basemfnodes : 0,
                                        calls : 1,
                                        mfnodes : 1},
                        read : { bytes : 2384},
                        write : { bytes : 1003}}}}
  $ cd client1
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > 
  > [treemanifest]
  > treeonly=True
  > sendtrees=True
  > EOF

Add a few more public server comits
  $ cd ../master
  $ mkcommit c
  $ mkcommit d
  $ mkcommit e
  $ hg phase -p -r 'all()'

Create an extension the prints out whenever we compare manifests on the server
  $ cat > "$TESTTMP/diffdebug.py" << EOF
  > from edenscm.mercurial import manifest
  > class manifestdict(manifest.manifestdict):
  >     ui = None
  >     def diff(self, *args, **kwargs):
  >         if self.ui:
  >             self.ui.warn("*** manifestdict is comparing manifests\n")
  >         return super(manifestdict, self).diff(*args, **kwargs)
  > def extsetup(ui):
  >     manifestdict.ui = ui
  >     manifest.manifestdict = manifestdict
  > EOF
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > diffdebug=$TESTTMP/diffdebug.py
  > EOF

Pull exactly up to d into the client
  $ cd ../client1
  $ hg pull -r 055a42cdd887
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  new changesets 177f92b77385:055a42cdd887
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 1155},
                        write : { bytes : 693}}}}

