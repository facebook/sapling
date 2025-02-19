#modern-config-incompatible

#require no-eden

#chg-compatible


  $ . "$TESTDIR/library.sh"

Set up an extension to make sure remotefilelog clientsetup() runs
unconditionally even if we have never used a local shallow repo.
This mimics behavior when using remotefilelog with chg.  clientsetup() can be
triggered due to a shallow repo, and then the code can later interact with
non-shallow repositories.

  $ cat > setupremotefilelog.py << EOF
  > from sapling import extensions
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
  $ hg book master

  $ cd ..

  $ hg clone ssh://user@dummy/master child -q

We should see the remotefilelog capability here, which advertises that
the server supports our custom getfiles method.

  $ cd master
  $ echo 'hello' | hg -R . serve --stdio
  * (glob)
  capabilities: *getfile* (glob)
  $ echo 'capabilities' | hg -R . serve --stdio ; echo
  * (glob)
  *getfile* (glob)

Pull to the child repository.  Use our custom setupremotefilelog extension
to ensure that remotefilelog.onetimeclientsetup() gets triggered.  (Without
using chg it normally would not be run in this case since the local repository
is not shallow.)

  $ echo y > y
  $ hg commit -qAm y
  $ hg book -r . master

  $ cd ../child
  $ hg pull -q --config extensions.setuprfl=$TESTTMP/setupremotefilelog.py

  $ hg up master
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat y
  y

Test that bundle works in a non-remotefilelog repo w/ remotefilelog loaded

  $ echo y >> y
  $ hg commit -qAm "modify y"
  $ hg bundle --base ".^" --rev . mybundle.hg --config extensions.setuprfl=$TESTTMP/setupremotefilelog.py
  1 changesets found

  $ cd ..
