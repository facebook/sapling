#chg-compatible

  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

Set up an extension to make sure remotefilelog clientsetup() runs
unconditionally even if we have never used a local shallow repo.
This mimics behavior when using remotefilelog with chg.  clientsetup() can be
triggered due to a shallow repo, and then the code can later interact with
non-shallow repositories.

  $ cat > setupremotefilelog.py << EOF
  > from edenscm.mercurial import extensions
  > def extsetup(ui):
  >     remotefilelog = extensions.find('remotefilelog')
  >     remotefilelog.onetimeclientsetup(ui)
  > EOF

Set up the master repository to pull from.

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x

  $ cd ..

  $ hg clone ssh://user@dummy/master child -q

We should see the remotefilelog capability here, which advertises that
the server supports our custom getfiles method.

  $ cd master
  $ echo 'hello' | hg -R . serve --stdio
  * (glob)
  capabilities: lookup * remotefilelog getflogheads getfile (glob)
  $ echo 'capabilities' | hg -R . serve --stdio ; echo
  * (glob)
  * remotefilelog getflogheads getfile (glob)

Pull to the child repository.  Use our custom setupremotefilelog extension
to ensure that remotefilelog.onetimeclientsetup() gets triggered.  (Without
using chg it normally would not be run in this case since the local repository
is not shallow.)

  $ echo y > y
  $ hg commit -qAm y

  $ cd ../child
  $ hg pull --config extensions.setuprfl=$TESTTMP/setupremotefilelog.py
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets d34c38483be9

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat y
  y

Test that bundle works in a non-remotefilelog repo w/ remotefilelog loaded

  $ echo y >> y
  $ hg commit -qAm "modify y"
  $ hg bundle --base ".^" --rev . mybundle.hg --config extensions.setuprfl=$TESTTMP/setupremotefilelog.py
  1 changesets found

  $ cd ..
