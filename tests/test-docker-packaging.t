#require test-repo slow docker

Ensure debuild doesn't run the testsuite, as that could get silly.
  $ DEB_BUILD_OPTIONS=nocheck
  $ export DEB_BUILD_OPTIONS
  $ OUTPUTDIR=`pwd`
  $ export OUTPUTDIR

  $ cd "$TESTDIR"/..
  $ make docker-debian-jessie > $OUTPUTDIR/build.log 2>&1
  $ cd $OUTPUTDIR
  $ ls *.deb
  mercurial-common_*.deb (glob)
  mercurial_*.deb (glob)

We check debian package contents with portable tools so that when
we're on non-debian machines we can still test the packages that are
built using docker.

main deb should have .so but no .py
  $ ar x mercurial_*.deb
  $ tar tf data.tar* | egrep '(localrepo|parsers)'
  ./usr/lib/python2.7/dist-packages/mercurial/parsers*.so (glob)
mercurial-common should have .py but no .so or .pyc
  $ ar x mercurial-common_*.deb
  $ tar tf data.tar* | egrep '(localrepo|parsers)'
  ./usr/lib/python2.7/dist-packages/mercurial/pure/parsers.py
  ./usr/lib/python2.7/dist-packages/mercurial/localrepo.py
