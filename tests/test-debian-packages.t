#require test-repo slow debhelper

Ensure debuild doesn't run the testsuite, as that could get silly.
  $ DEB_BUILD_OPTIONS=nocheck
  $ export DEB_BUILD_OPTIONS
  $ OUTPUTDIR=`pwd`
  $ export OUTPUTDIR

  $ cd "$TESTDIR"/..
  $ make deb > $OUTPUTDIR/build.log 2>&1
  $ cd $OUTPUTDIR
  $ ls *.deb
  mercurial-common_*.deb (glob)
  mercurial_*.deb (glob)
main deb should have .so but no .py
  $ dpkg --contents mercurial_*.deb | egrep '(localrepo|parsers)'
  * ./usr/lib/python2.7/dist-packages/mercurial/parsers*.so (glob)
mercurial-common should have py but no .so or pyc
  $ dpkg --contents mercurial-common_*.deb | egrep '(localrepo|parsers)'
  * ./usr/lib/python2.7/dist-packages/mercurial/localrepo.py (glob)
