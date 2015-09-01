#require test-repo slow docker

  $ OUTPUTDIR=`pwd`
  $ export OUTPUTDIR

  $ cd "$TESTDIR"/..
  $ make docker-debian-jessie > $OUTPUTDIR/build.log 2>&1
  $ cd $OUTPUTDIR
  $ ls *.deb
  mercurial-*.deb (glob)

We check debian package contents with portable tools so that when
we're on non-debian machines we can still test the packages that are
built using docker.
  $ ar x mercurial*.deb
  $ tar tf data.tar* | grep localrepo | sort
  ./usr/lib/python2.7/site-packages/mercurial/localrepo.py
  ./usr/lib/python2.7/site-packages/mercurial/localrepo.pyc
  $ rm -f *.deb build.log
