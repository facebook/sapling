#require test-repo slow osx bdistmpkg
  $ OUTPUTDIR=`pwd`
  $ export OUTPUTDIR
  $ KEEPMPKG=yes
  $ export KEEPMPKG

  $ cd "$TESTDIR"/..
  $ make osx > $OUTPUTDIR/build.log 2>&1
  $ cd $OUTPUTDIR
  $ ls -d *.dmg *.mpkg
  mercurial-*-macosx10.*.dmg (glob)
  mercurial-*-macosx10.*.mpkg (glob)

Gather list of all installed files:
  $ find *.mpkg -name Archive.bom | xargs lsbom > boms.txt

TODO: update to -f 1,2,3 when we're confident the installed owner of
our files is corect. Right now it looks like it's the id of the user
that builds the mpkg, which is probably slightly wrong.

Spot-check some randomly selected files:
  $ grep bdiff boms.txt | cut -d '	' -f 1,2
  ./mercurial/bdiff.so	100775
  ./mercurial/pure/bdiff.py	100664
  ./mercurial/pure/bdiff.pyc	100664
  ./mercurial/pure/bdiff.pyo	100664
TODO: man pages don't get installed
  $ egrep 'man[15]' boms.txt | cut -d '	' -f 1,2
  $ grep bser boms.txt | cut -d '	' -f 1,2
  ./hgext/fsmonitor/pywatchman/bser.so	100775
  ./hgext/fsmonitor/pywatchman/pybser.py	100664
  ./hgext/fsmonitor/pywatchman/pybser.pyc	100664
  ./hgext/fsmonitor/pywatchman/pybser.pyo	100664
  $ grep localrepo boms.txt | cut -d '	' -f 1,2
  ./mercurial/localrepo.py	100664
  ./mercurial/localrepo.pyc	100664
  ./mercurial/localrepo.pyo	100664
  $ grep '/hg	' boms.txt | cut -d '	' -f 1,2
  ./hg	100775

Note that we're not currently installing any /etc/mercurial stuff,
including merge-tool configurations.
