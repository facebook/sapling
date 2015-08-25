#require test-repo slow debhelper
  $ OUTPUTDIR=`pwd`
  $ export OUTPUTDIR

  $ cd "$TESTDIR"/..
  $ make deb > $OUTPUTDIR/build.log 2>&1
  $ cd $OUTPUTDIR
  $ ls *.deb
  mercurial-*.deb (glob)
  $ dpkg --contents mercurial*.deb | grep localrepo
  * ./usr/lib/python2.7/site-packages/mercurial/localrepo.py (glob)
  * ./usr/lib/python2.7/site-packages/mercurial/localrepo.pyc (glob)
  $ rm -f *.deb build.log
