Test config convert.cvsps.mergefrom config setting.
(Should test similar mergeto feature, but I don't understand it yet.)
Requires builtin cvsps.

  $ "$TESTDIR/hghave" cvs || exit 80
  $ CVSROOT=`pwd`/cvsrepo
  $ export CVSROOT

  $ cvscall()
  > {
  >     cvs -f "$@"
  > }

output of 'cvs ci' varies unpredictably, so just discard it
XXX copied from test-convert-cvs-synthetic

  $ cvsci()
  > {
  >     sleep 1
  >     cvs -f ci "$@" > /dev/null
  > }

XXX copied from test-convert-cvs-synthetic

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert = " >> $HGRCPATH
  $ echo "graphlog = " >> $HGRCPATH
  $ echo "[convert]" >> $HGRCPATH
  $ echo "cvsps.cache=0" >> $HGRCPATH
  $ echo "cvsps.mergefrom=\[MERGE from (\S+)\]" >> $HGRCPATH

create cvs repository with one project

  $ mkdir cvsrepo
  $ cvscall -q -d "$CVSROOT" init
  $ mkdir cvsrepo/proj

populate cvs repository

  $ cvscall -Q co proj
  $ cd proj
  $ touch file1
  $ cvscall -Q add file1
  $ cvsci -m"add file1 on trunk"
  cvs commit: Examining .

create two release branches

  $ cvscall -q tag -b v1_0
  T file1
  $ cvscall -q tag -b v1_1
  T file1

modify file1 on branch v1_0

  $ cvscall -Q update -rv1_0
  $ sleep 1
  $ echo "change" >> file1
  $ cvsci -m"add text"
  cvs commit: Examining .

make unrelated change on v1_1

  $ cvscall -Q update -rv1_1
  $ touch unrelated
  $ cvscall -Q add unrelated
  $ cvsci -m"unrelated change"
  cvs commit: Examining .

merge file1 to v1_1

  $ cvscall -Q update -jv1_0
  RCS file: $TESTTMP/cvsrepo/proj/file1,v
  retrieving revision 1.1
  retrieving revision 1.1.2.1
  Merging differences between 1.1 and 1.1.2.1 into file1
  $ cvsci -m"add text [MERGE from v1_0]"
  cvs commit: Examining .

merge change to trunk

  $ cvscall -Q update -A
  $ cvscall -Q update -jv1_1
  RCS file: $TESTTMP/cvsrepo/proj/file1,v
  retrieving revision 1.1
  retrieving revision 1.1.4.1
  Merging differences between 1.1 and 1.1.4.1 into file1
  $ cvsci -m"add text [MERGE from v1_1]"
  cvs commit: Examining .

non-merged change on trunk

  $ echo "foo" > file2
  $ cvscall -Q add file2
  $ cvsci -m"add file2 on trunk" file2

this will create rev 1.3
change on trunk to backport

  $ echo "backport me" >> file1
  $ cvsci -m"add other text" file1
  $ cvscall log file1
  
  RCS file: $TESTTMP/cvsrepo/proj/file1,v
  Working file: file1
  head: 1.3
  branch:
  locks: strict
  access list:
  symbolic names:
  	v1_1: 1.1.0.4
  	v1_0: 1.1.0.2
  keyword substitution: kv
  total revisions: 5;	selected revisions: 5
  description:
  ----------------------------
  revision 1.3
  date: * (glob)
  add other text
  ----------------------------
  revision 1.2
  date: * (glob)
  add text [MERGE from v1_1]
  ----------------------------
  revision 1.1
  date: * (glob)
  branches:  1.1.2;  1.1.4;
  add file1 on trunk
  ----------------------------
  revision 1.1.4.1
  date: * (glob)
  add text [MERGE from v1_0]
  ----------------------------
  revision 1.1.2.1
  date: * (glob)
  add text
  =============================================================================

XXX how many ways are there to spell "trunk" with CVS?
backport trunk change to v1_1

  $ cvscall -Q update -rv1_1
  $ cvscall -Q update -j1.2 -j1.3 file1
  RCS file: $TESTTMP/cvsrepo/proj/file1,v
  retrieving revision 1.2
  retrieving revision 1.3
  Merging differences between 1.2 and 1.3 into file1
  $ cvsci -m"add other text [MERGE from HEAD]" file1

fix bug on v1_1, merge to trunk with error

  $ cvscall -Q update -rv1_1
  $ echo "merge forward" >> file1
  $ cvscall -Q tag unmerged
  $ cvsci -m"fix file1"
  cvs commit: Examining .
  $ cvscall -Q update -A
  $ cvscall -Q update -junmerged -jv1_1
  RCS file: $TESTTMP/cvsrepo/proj/file1,v
  retrieving revision 1.1.4.2
  retrieving revision 1.1.4.3
  Merging differences between 1.1.4.2 and 1.1.4.3 into file1

note the typo in the commit log message

  $ cvsci -m"fix file1 [MERGE from v1-1]"
  cvs commit: Examining .
  $ cvs -Q tag -d unmerged

convert to hg

  $ cd ..
  $ hg convert proj proj.hg
  initializing destination proj.hg repository
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  12 log entries
  creating changesets
  warning: CVS commit message references non-existent branch 'v1-1':
  fix file1 [MERGE from v1-1]
  10 changeset entries
  sorting...
  converting...
  9 add file1 on trunk
  8 unrelated change
  7 add text
  6 add text [MERGE from v1_0]
  5 add text [MERGE from v1_1]
  4 add file2 on trunk
  3 add other text
  2 add other text [MERGE from HEAD]
  1 fix file1
  0 fix file1 [MERGE from v1-1]

complete log

  $ template="{rev}: '{branches}' {desc}\n"
  $ hg -R proj.hg log --template="$template"
  9: '' fix file1 [MERGE from v1-1]
  8: 'v1_1' fix file1
  7: 'v1_1' add other text [MERGE from HEAD]
  6: '' add other text
  5: '' add file2 on trunk
  4: '' add text [MERGE from v1_1]
  3: 'v1_1' add text [MERGE from v1_0]
  2: 'v1_0' add text
  1: 'v1_1' unrelated change
  0: '' add file1 on trunk

graphical log

  $ hg -R proj.hg glog --template="$template"
  o  9: '' fix file1 [MERGE from v1-1]
  |
  | o  8: 'v1_1' fix file1
  | |
  | o  7: 'v1_1' add other text [MERGE from HEAD]
  |/|
  o |  6: '' add other text
  | |
  o |  5: '' add file2 on trunk
  | |
  o |  4: '' add text [MERGE from v1_1]
  |\|
  | o    3: 'v1_1' add text [MERGE from v1_0]
  | |\
  +---o  2: 'v1_0' add text
  | |
  | o  1: 'v1_1' unrelated change
  |/
  o  0: '' add file1 on trunk
  
