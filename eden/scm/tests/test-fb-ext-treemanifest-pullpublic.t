  $ . "$TESTDIR/library.sh"
  $ setconfig experimental.allowfilepeer=True
  $ setconfig treemanifest.treeonly=False

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > treemanifest=$TESTDIR/../sapling/ext/treemanifestserver.py
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > EOF
  $ mkcommit a
  $ mkcommit b
  $ hg debugmakepublic -r 'all()'

Clone it
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master client1 -q --config extensions.treemanifest= --config treemanifest.treeonly=True
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
  $ cp -R . ../master-lagged
  $ mkcommit d
  $ mkcommit e
  $ hg debugmakepublic -r 'all()'

Create an extension the prints out whenever we compare manifests on the server
  $ cat > "$TESTTMP/diffdebug.py" << EOF
  > from sapling import manifest
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

Test error message about MissingNodesError
  $ drawdag --config paths.default=ssh://user@dummy/master-lagged --config remotefilelog.debug=0 --config devel.print-metrics=0 << 'EOS'
  > x
  > |
  > tip
  > EOS
  abort: "unable to find the following nodes locally or on the server: ('', f064a7f8e3e138341587096641d86e9d23cd9778)"
  (commit: 055a42cdd88768532f9cf79daa407fc8d138de9b)
  [255]
