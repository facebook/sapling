#require cvs

  $ cvscall()
  > {
  >     cvs -f "$@"
  > }
  $ hgcat()
  > {
  >     hg --cwd src-hg cat -r tip "$1"
  > }
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert = " >> $HGRCPATH
  $ cat > cvshooks.py <<EOF
  > def cvslog(ui,repo,hooktype,log):
  >     print "%s hook: %d entries"%(hooktype,len(log))
  > 
  > def cvschangesets(ui,repo,hooktype,changesets):
  >     print "%s hook: %d changesets"%(hooktype,len(changesets))
  > EOF
  $ hookpath=`pwd`
  $ cat <<EOF >> $HGRCPATH
  > [hooks]
  > cvslog = python:$hookpath/cvshooks.py:cvslog
  > cvschangesets = python:$hookpath/cvshooks.py:cvschangesets
  > EOF

create cvs repository

  $ mkdir cvsrepo
  $ cd cvsrepo
  $ CVSROOT=`pwd`
  $ export CVSROOT
  $ CVS_OPTIONS=-f
  $ export CVS_OPTIONS
  $ cd ..
  $ rmdir cvsrepo
  $ cvscall -q -d "$CVSROOT" init

create source directory

  $ mkdir src-temp
  $ cd src-temp
  $ echo a > a
  $ mkdir b
  $ cd b
  $ echo c > c
  $ cd ..

import source directory

  $ cvscall -q import -m import src INITIAL start
  N src/a
  N src/b/c
  
  No conflicts created by this import
  
  $ cd ..

checkout source directory

  $ cvscall -q checkout src
  U src/a
  U src/b/c

commit a new revision changing b/c

  $ cd src
  $ sleep 1
  $ echo c >> b/c
  $ cvscall -q commit -mci0 . | grep '<--'
  $TESTTMP/cvsrepo/src/b/c,v  <--  *c (glob)
  $ cd ..

convert fresh repo and also check localtimezone option

NOTE: This doesn't check all time zones -- it merely determines that
the configuration option is taking effect.

An arbitrary (U.S.) time zone is used here.  TZ=US/Hawaii is selected
since it does not use DST (unlike other U.S. time zones) and is always
a fixed difference from UTC.

  $ TZ=US/Hawaii hg convert --config convert.localtimezone=True src src-hg
  initializing destination src-hg repository
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  5 log entries
  cvslog hook: 5 entries
  creating changesets
  3 changeset entries
  cvschangesets hook: 3 changesets
  sorting...
  converting...
  2 Initial revision
  1 ci0
  0 import
  updating tags
  $ hgcat a
  a
  $ hgcat b/c
  c
  c

convert fresh repo with --filemap

  $ echo include b/c > filemap
  $ hg convert --filemap filemap src src-filemap
  initializing destination src-filemap repository
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  5 log entries
  cvslog hook: 5 entries
  creating changesets
  3 changeset entries
  cvschangesets hook: 3 changesets
  sorting...
  converting...
  2 Initial revision
  1 ci0
  0 import
  filtering out empty revision
  repository tip rolled back to revision 1 (undo convert)
  updating tags
  $ hgcat b/c
  c
  c
  $ hg -R src-filemap log --template '{rev} {desc} files: {files}\n'
  2 update tags files: .hgtags
  1 ci0 files: b/c
  0 Initial revision files: b/c

convert full repository (issue1649)

  $ cvscall -q -d "$CVSROOT" checkout -d srcfull "." | grep -v CVSROOT
  U srcfull/src/a
  U srcfull/src/b/c
  $ ls srcfull
  CVS
  CVSROOT
  src
  $ hg convert srcfull srcfull-hg \
  >     | grep -v 'log entries' | grep -v 'hook:' \
  >     | grep -v '^[0-3] .*' # filter instable changeset order
  initializing destination srcfull-hg repository
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  creating changesets
  4 changeset entries
  sorting...
  converting...
  updating tags
  $ hg cat -r tip --cwd srcfull-hg src/a
  a
  $ hg cat -r tip --cwd srcfull-hg src/b/c
  c
  c

commit new file revisions

  $ cd src
  $ echo a >> a
  $ echo c >> b/c
  $ cvscall -q commit -mci1 . | grep '<--'
  $TESTTMP/cvsrepo/src/a,v  <--  a
  $TESTTMP/cvsrepo/src/b/c,v  <--  *c (glob)
  $ cd ..

convert again

  $ TZ=US/Hawaii hg convert --config convert.localtimezone=True src src-hg
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  7 log entries
  cvslog hook: 7 entries
  creating changesets
  4 changeset entries
  cvschangesets hook: 4 changesets
  sorting...
  converting...
  0 ci1
  $ hgcat a
  a
  a
  $ hgcat b/c
  c
  c
  c

convert again with --filemap

  $ hg convert --filemap filemap src src-filemap
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  7 log entries
  cvslog hook: 7 entries
  creating changesets
  4 changeset entries
  cvschangesets hook: 4 changesets
  sorting...
  converting...
  0 ci1
  $ hgcat b/c
  c
  c
  c
  $ hg -R src-filemap log --template '{rev} {desc} files: {files}\n'
  3 ci1 files: b/c
  2 update tags files: .hgtags
  1 ci0 files: b/c
  0 Initial revision files: b/c

commit branch

  $ cd src
  $ cvs -q update -r1.1 b/c
  U b/c
  $ cvs -q tag -b branch
  T a
  T b/c
  $ cvs -q update -r branch > /dev/null
  $ echo d >> b/c
  $ cvs -q commit -mci2 . | grep '<--'
  $TESTTMP/cvsrepo/src/b/c,v  <--  *c (glob)
  $ cd ..

convert again

  $ TZ=US/Hawaii hg convert --config convert.localtimezone=True src src-hg
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  8 log entries
  cvslog hook: 8 entries
  creating changesets
  5 changeset entries
  cvschangesets hook: 5 changesets
  sorting...
  converting...
  0 ci2
  $ hgcat b/c
  c
  d

convert again with --filemap

  $ TZ=US/Hawaii hg convert --config convert.localtimezone=True --filemap filemap src src-filemap
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  8 log entries
  cvslog hook: 8 entries
  creating changesets
  5 changeset entries
  cvschangesets hook: 5 changesets
  sorting...
  converting...
  0 ci2
  $ hgcat b/c
  c
  d
  $ hg -R src-filemap log --template '{rev} {desc} files: {files}\n'
  4 ci2 files: b/c
  3 ci1 files: b/c
  2 update tags files: .hgtags
  1 ci0 files: b/c
  0 Initial revision files: b/c

commit a new revision with funny log message

  $ cd src
  $ sleep 1
  $ echo e >> a
  $ cvscall -q commit -m'funny
  > ----------------------------
  > log message' . | grep '<--' |\
  >  sed -e 's:.*src/\(.*\),v.*:checking in src/\1,v:g'
  checking in src/a,v

commit new file revisions with some fuzz

  $ sleep 1
  $ echo f >> a
  $ cvscall -q commit -mfuzzy . | grep '<--'
  $TESTTMP/cvsrepo/src/a,v  <--  a
  $ sleep 4 # the two changes will be split if fuzz < 4
  $ echo g >> b/c
  $ cvscall -q commit -mfuzzy . | grep '<--'
  $TESTTMP/cvsrepo/src/b/c,v  <--  *c (glob)
  $ cd ..

convert again

  $ TZ=US/Hawaii hg convert --config convert.cvsps.fuzz=2 --config convert.localtimezone=True src src-hg
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  11 log entries
  cvslog hook: 11 entries
  creating changesets
  8 changeset entries
  cvschangesets hook: 8 changesets
  sorting...
  converting...
  2 funny
  1 fuzzy
  0 fuzzy
  $ hg -R src-hg log -G --template '{rev} ({branches}) {desc} date: {date|date} files: {files}\n'
  o  8 (branch) fuzzy date: * -1000 files: b/c (glob)
  |
  o  7 (branch) fuzzy date: * -1000 files: a (glob)
  |
  o  6 (branch) funny
  |  ----------------------------
  |  log message date: * -1000 files: a (glob)
  o  5 (branch) ci2 date: * -1000 files: b/c (glob)
  
  o  4 () ci1 date: * -1000 files: a b/c (glob)
  |
  o  3 () update tags date: * +0000 files: .hgtags (glob)
  |
  | o  2 (INITIAL) import date: * -1000 files: (glob)
  | |
  o |  1 () ci0 date: * -1000 files: b/c (glob)
  |/
  o  0 () Initial revision date: * -1000 files: a b/c (glob)
  

testing debugcvsps

  $ cd src
  $ hg debugcvsps --fuzz=2
  collecting CVS rlog
  11 log entries
  cvslog hook: 11 entries
  creating changesets
  10 changeset entries
  cvschangesets hook: 10 changesets
  ---------------------
  PatchSet 1 
  Date: * (glob)
  Author: * (glob)
  Branch: HEAD
  Tag: (none) 
  Branchpoints: INITIAL 
  Log:
  Initial revision
  
  Members: 
  	a:INITIAL->1.1 
  
  ---------------------
  PatchSet 2 
  Date: * (glob)
  Author: * (glob)
  Branch: HEAD
  Tag: (none) 
  Branchpoints: INITIAL, branch 
  Log:
  Initial revision
  
  Members: 
  	b/c:INITIAL->1.1 
  
  ---------------------
  PatchSet 3 
  Date: * (glob)
  Author: * (glob)
  Branch: INITIAL
  Tag: start 
  Log:
  import
  
  Members: 
  	a:1.1->1.1.1.1 
  	b/c:1.1->1.1.1.1 
  
  ---------------------
  PatchSet 4 
  Date: * (glob)
  Author: * (glob)
  Branch: HEAD
  Tag: (none) 
  Log:
  ci0
  
  Members: 
  	b/c:1.1->1.2 
  
  ---------------------
  PatchSet 5 
  Date: * (glob)
  Author: * (glob)
  Branch: HEAD
  Tag: (none) 
  Branchpoints: branch 
  Log:
  ci1
  
  Members: 
  	a:1.1->1.2 
  
  ---------------------
  PatchSet 6 
  Date: * (glob)
  Author: * (glob)
  Branch: HEAD
  Tag: (none) 
  Log:
  ci1
  
  Members: 
  	b/c:1.2->1.3 
  
  ---------------------
  PatchSet 7 
  Date: * (glob)
  Author: * (glob)
  Branch: branch
  Tag: (none) 
  Log:
  ci2
  
  Members: 
  	b/c:1.1->1.1.2.1 
  
  ---------------------
  PatchSet 8 
  Date: * (glob)
  Author: * (glob)
  Branch: branch
  Tag: (none) 
  Log:
  funny
  ----------------------------
  log message
  
  Members: 
  	a:1.2->1.2.2.1 
  
  ---------------------
  PatchSet 9 
  Date: * (glob)
  Author: * (glob)
  Branch: branch
  Tag: (none) 
  Log:
  fuzzy
  
  Members: 
  	a:1.2.2.1->1.2.2.2 
  
  ---------------------
  PatchSet 10 
  Date: * (glob)
  Author: * (glob)
  Branch: branch
  Tag: (none) 
  Log:
  fuzzy
  
  Members: 
  	b/c:1.1.2.1->1.1.2.2 
  

  $ cd ..
