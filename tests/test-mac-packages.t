#require test-repo slow osx osxpackaging
  $ OUTPUTDIR=`pwd`
  $ export OUTPUTDIR
  $ KEEPMPKG=yes
  $ export KEEPMPKG

  $ cd "$TESTDIR"/..
  $ rm -rf dist
  $ make osx > $OUTPUTDIR/build.log 2>&1
  $ cd $OUTPUTDIR
  $ ls -d *.pkg
  Mercurial-*-macosx10.*.pkg (glob)

  $ xar -xf Mercurial*.pkg

Gather list of all installed files:
  $ lsbom mercurial.pkg/Bom > boms.txt

Spot-check some randomly selected files:
  $ grep bdiff boms.txt | cut -d '	' -f 1,2,3
  ./Library/Python/2.7/site-packages/mercurial/bdiff.so	100755	0/0
  ./Library/Python/2.7/site-packages/mercurial/pure/bdiff.py	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/pure/bdiff.pyc	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/pure/bdiff.pyo	100644	0/0
  $ egrep 'man[15]' boms.txt | cut -d '	' -f 1,2,3
  ./usr/local/share/man/man1	40755	0/0
  ./usr/local/share/man/man1/hg.1	100644	0/0
  ./usr/local/share/man/man5	40755	0/0
  ./usr/local/share/man/man5/hgignore.5	100644	0/0
  ./usr/local/share/man/man5/hgrc.5	100644	0/0
  $ grep bser boms.txt | cut -d '	' -f 1,2,3
  ./Library/Python/2.7/site-packages/hgext/fsmonitor/pywatchman/bser.so	100755	0/0
  ./Library/Python/2.7/site-packages/hgext/fsmonitor/pywatchman/pybser.py	100644	0/0
  ./Library/Python/2.7/site-packages/hgext/fsmonitor/pywatchman/pybser.pyc	100644	0/0
  ./Library/Python/2.7/site-packages/hgext/fsmonitor/pywatchman/pybser.pyo	100644	0/0
  $ grep localrepo boms.txt | cut -d '	' -f 1,2,3
  ./Library/Python/2.7/site-packages/mercurial/localrepo.py	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/localrepo.pyc	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/localrepo.pyo	100644	0/0
  $ grep '/hg	' boms.txt | cut -d '	' -f 1,2,3
  ./usr/local/bin/hg	100755	0/0

Note that we're not currently installing any /etc/mercurial stuff,
including merge-tool configurations.
