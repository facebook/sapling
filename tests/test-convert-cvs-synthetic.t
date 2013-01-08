This feature requires use of builtin cvsps!

  $ "$TESTDIR/hghave" cvs || exit 80
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert = " >> $HGRCPATH
  $ echo "graphlog = " >> $HGRCPATH

create cvs repository with one project

  $ mkdir cvsrepo
  $ cd cvsrepo
  $ CVSROOT=`pwd`
  $ export CVSROOT
  $ CVS_OPTIONS=-f
  $ export CVS_OPTIONS
  $ cd ..
  $ cvscall()
  > {
  >     cvs -f "$@"
  > }

output of 'cvs ci' varies unpredictably, so just discard it

  $ cvsci()
  > {
  >     sleep 1
  >     cvs -f ci "$@" >/dev/null
  > }
  $ cvscall -d "$CVSROOT" init
  $ mkdir cvsrepo/proj
  $ cvscall -q co proj

create file1 on the trunk

  $ cd proj
  $ touch file1
  $ cvscall -Q add file1
  $ cvsci -m"add file1 on trunk" file1

create two branches

  $ cvscall -q tag -b v1_0
  T file1
  $ cvscall -q tag -b v1_1
  T file1

create file2 on branch v1_0

  $ cvscall -Q up -rv1_0
  $ touch file2
  $ cvscall -Q add file2
  $ cvsci -m"add file2" file2

create file3, file4 on branch v1_1

  $ cvscall -Q up -rv1_1
  $ touch file3
  $ touch file4
  $ cvscall -Q add file3 file4
  $ cvsci -m"add file3, file4 on branch v1_1" file3 file4

merge file2 from v1_0 to v1_1

  $ cvscall -Q up -jv1_0
  $ cvsci -m"MERGE from v1_0: add file2"
  cvs commit: Examining .

Step things up a notch: now we make the history really hairy, with
changes bouncing back and forth between trunk and v1_2 and merges
going both ways.  (I.e., try to model the real world.)
create branch v1_2

  $ cvscall -Q up -A
  $ cvscall -q tag -b v1_2
  T file1

create file5 on branch v1_2

  $ cvscall -Q up -rv1_2
  $ touch file5
  $ cvs -Q add file5
  $ cvsci -m"add file5 on v1_2"
  cvs commit: Examining .

create file6 on trunk post-v1_2

  $ cvscall -Q up -A
  $ touch file6
  $ cvscall -Q add file6
  $ cvsci -m"add file6 on trunk post-v1_2"
  cvs commit: Examining .

merge file5 from v1_2 to trunk

  $ cvscall -Q up -A
  $ cvscall -Q up -jv1_2 file5
  $ cvsci -m"MERGE from v1_2: add file5"
  cvs commit: Examining .

merge file6 from trunk to v1_2

  $ cvscall -Q up -rv1_2
  $ cvscall up -jHEAD file6
  U file6
  $ cvsci -m"MERGE from HEAD: add file6"
  cvs commit: Examining .

cvs rlog output

  $ cvscall -q rlog proj | egrep '^(RCS file|revision)'
  RCS file: $TESTTMP/cvsrepo/proj/file1,v
  revision 1.1
  RCS file: $TESTTMP/cvsrepo/proj/Attic/file2,v
  revision 1.1
  revision 1.1.4.2
  revision 1.1.4.1
  revision 1.1.2.1
  RCS file: $TESTTMP/cvsrepo/proj/Attic/file3,v
  revision 1.1
  revision 1.1.2.1
  RCS file: $TESTTMP/cvsrepo/proj/Attic/file4,v
  revision 1.1
  revision 1.1.2.1
  RCS file: $TESTTMP/cvsrepo/proj/file5,v
  revision 1.2
  revision 1.1
  revision 1.1.2.1
  RCS file: $TESTTMP/cvsrepo/proj/file6,v
  revision 1.1
  revision 1.1.2.2
  revision 1.1.2.1

convert to hg (#1)

  $ cd ..
  $ hg convert --datesort proj proj.hg
  initializing destination proj.hg repository
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  15 log entries
  creating changesets
  9 changeset entries
  sorting...
  converting...
  8 add file1 on trunk
  7 add file2
  6 MERGE from v1_0: add file2
  5 file file3 was initially added on branch v1_1.
  4 add file3, file4 on branch v1_1
  3 add file5 on v1_2
  2 add file6 on trunk post-v1_2
  1 MERGE from HEAD: add file6
  0 MERGE from v1_2: add file5

hg glog output (#1)

  $ hg -R proj.hg glog --template "{rev} {desc}\n"
  o  8 MERGE from v1_2: add file5
  |
  | o  7 MERGE from HEAD: add file6
  | |
  o |  6 add file6 on trunk post-v1_2
  | |
  | o  5 add file5 on v1_2
  | |
  | | o  4 add file3, file4 on branch v1_1
  | | |
  o | |  3 file file3 was initially added on branch v1_1.
  |/ /
  | o  2 MERGE from v1_0: add file2
  |/
  | o  1 add file2
  |/
  o  0 add file1 on trunk
  

convert to hg (#2: with merge detection)

  $ hg convert \
  >   --config convert.cvsps.mergefrom='"^MERGE from (\S+):"' \
  >   --datesort \
  >   proj proj.hg2
  initializing destination proj.hg2 repository
  connecting to $TESTTMP/cvsrepo
  scanning source...
  collecting CVS rlog
  15 log entries
  creating changesets
  9 changeset entries
  sorting...
  converting...
  8 add file1 on trunk
  7 add file2
  6 MERGE from v1_0: add file2
  5 file file3 was initially added on branch v1_1.
  4 add file3, file4 on branch v1_1
  3 add file5 on v1_2
  2 add file6 on trunk post-v1_2
  1 MERGE from HEAD: add file6
  0 MERGE from v1_2: add file5

hg glog output (#2)

  $ hg -R proj.hg2 glog --template "{rev} {desc}\n"
  o  8 MERGE from v1_2: add file5
  |
  | o  7 MERGE from HEAD: add file6
  | |
  o |  6 add file6 on trunk post-v1_2
  | |
  | o  5 add file5 on v1_2
  | |
  | | o  4 add file3, file4 on branch v1_1
  | | |
  o | |  3 file file3 was initially added on branch v1_1.
  |/ /
  | o  2 MERGE from v1_0: add file2
  |/
  | o  1 add file2
  |/
  o  0 add file1 on trunk
  
