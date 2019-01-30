create verbosemmap.py
  $ cat << EOF > verbosemmap.py
  > # extension to make util.mmapread verbose
  > 
  > from __future__ import absolute_import
  > 
  > from edenscm.mercurial import (
  >     extensions,
  >     pycompat,
  >     util,
  > )
  > 
  > def extsetup(ui):
  >     def mmapread(orig, fp):
  >         ui.write(b"mmapping %s\n" % pycompat.bytestr(fp.name))
  >         ui.flush()
  >         return orig(fp)
  > 
  >     extensions.wrapfunction(util, 'mmapread', mmapread)
  > EOF

setting up base repo
  $ hg init a
  $ cd a
  $ touch a
  $ hg add a
  $ hg commit -qm base
  $ for i in `$TESTDIR/seq.py 1 100` ; do
  > echo $i > a
  > hg commit -qm $i
  > done

set up verbosemmap extension
  $ cat << EOF >> $HGRCPATH
  > [extensions]
  > verbosemmap=$TESTTMP/verbosemmap.py
  > EOF

mmap index which is now more than 4k long
  $ hg log -l 5 -T '{rev}\n' --config experimental.mmapindexthreshold=4k
  mmapping $TESTTMP/a/.hg/store/00changelog.i
  100
  99
  98
  97
  96

do not mmap index which is still less than 32k
  $ hg log -l 5 -T '{rev}\n' --config experimental.mmapindexthreshold=32k
  100
  99
  98
  97
  96

  $ cd ..
