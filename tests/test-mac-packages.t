#require test-repo slow osx osxpackaging

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ testrepohgenv

  $ OUTPUTDIR="`pwd`"
  $ export OUTPUTDIR
  $ KEEPMPKG=yes
  $ export KEEPMPKG

  $ cd "$TESTDIR"/..
  $ contrib/genosxversion.py --selftest ignoredarg
  $ make osx > "$OUTPUTDIR/build.log" 2>&1
  $ cd "$OUTPUTDIR"
  $ ls -d *.pkg
  Mercurial-*-macosx10.*.pkg (glob)

  $ xar -xf Mercurial*.pkg

Gather list of all installed files:
  $ lsbom mercurial.pkg/Bom > boms.txt

We've had problems with the filter logic in the past. Make sure no
.DS_Store files ended up in the final package:
  $ grep DS_S boms.txt
  [1]

Spot-check some randomly selected files:
  $ grep bdiff boms.txt | cut -d '	' -f 1,2,3
  ./Library/Python/2.7/site-packages/mercurial/cext/bdiff.so	100755	0/0
  ./Library/Python/2.7/site-packages/mercurial/cffi/bdiff.py	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/cffi/bdiff.pyc	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/cffi/bdiff.pyo	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/cffi/bdiffbuild.py	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/cffi/bdiffbuild.pyc	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/cffi/bdiffbuild.pyo	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/pure/bdiff.py	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/pure/bdiff.pyc	100644	0/0
  ./Library/Python/2.7/site-packages/mercurial/pure/bdiff.pyo	100644	0/0
  $ grep zsh/site-functions/_hg boms.txt | cut -d '	' -f 1,2,3
  ./usr/local/share/zsh/site-functions/_hg	100644	0/0
  $ grep hg-completion.bash boms.txt | cut -d '	' -f 1,2,3
  ./usr/local/hg/contrib/hg-completion.bash	100644	0/0
  $ egrep 'man[15]' boms.txt | cut -d '	' -f 1,2,3
  ./usr/local/share/man/man1	40755	0/0
  ./usr/local/share/man/man1/chg.1	100644	0/0
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
  $ egrep 'bin/' boms.txt | cut -d '	' -f 1,2,3
  ./usr/local/bin/chg	100755	0/0
  ./usr/local/bin/hg	100755	0/0

Make sure the built binary uses the system Python interpreter
  $ bsdtar xf mercurial.pkg/Payload usr/local/bin
Use a glob to find this to avoid check-code whining about a fixed path.
  $ head -n 1 usr/local/b?n/hg
  #!/System/Library/Frameworks/Python.framework/Versions/2.7/Resources/Python.app/Contents/MacOS/Python

Note that we're not currently installing any /etc/mercurial stuff,
including merge-tool configurations.
