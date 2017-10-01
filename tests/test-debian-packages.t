#require test-repo slow debhelper debdeps

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ testrepohgenv

Ensure debuild doesn't run the testsuite, as that could get silly.
  $ DEB_BUILD_OPTIONS=nocheck
  $ export DEB_BUILD_OPTIONS
  $ OUTPUTDIR=`pwd`
  $ export OUTPUTDIR

  $ cd "$TESTDIR"/..
  $ make deb > $OUTPUTDIR/build.log 2>&1
  $ cd $OUTPUTDIR
  $ ls *.deb | grep -v 'dbg'
  mercurial-common_*.deb (glob)
  mercurial_*.deb (glob)
main deb should have .so but no .py
  $ dpkg --contents mercurial_*.deb | egrep '(localrepo|parsers)'
  * ./usr/lib/python2.7/dist-packages/mercurial/cext/parsers*.so (glob)
mercurial-common should have py but no .so or pyc
  $ dpkg --contents mercurial-common_*.deb | egrep '(localrepo|parsers.*so)'
  * ./usr/lib/python2.7/dist-packages/mercurial/localrepo.py (glob)
zsh completions should be in the common package
  $ dpkg --contents mercurial-common_*.deb | egrep 'zsh.*[^/]$'
  * ./usr/share/zsh/vendor-completions/_hg (glob)
chg should be installed alongside hg, in the 'mercurial' package
  $ dpkg --contents mercurial_*.deb | egrep 'chg$'
  * ./usr/bin/chg (glob)
chg should come with a man page
  $ dpkg --contents mercurial_*.deb | egrep 'man.*chg'
  * ./usr/share/man/man1/chg.1.gz (glob)
