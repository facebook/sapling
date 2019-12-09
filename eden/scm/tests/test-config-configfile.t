#chg-compatible

  $ hg init repo
  $ cd repo

Empty
  $ hg log --configfile | head -1
  hg log: option --configfile requires argument
  (use 'hg log -h' to get help)

Simple file
  $ cat >> $TESTTMP/simple.rc <<EOF
  > [mysection]
  > myname = myvalue
  > EOF
  $ hg config --configfile $TESTTMP/simple.rc mysection
  mysection.myname=myvalue

RC file that includes another
  $ cat >> $TESTTMP/include.rc <<EOF
  > [includesection]
  > includename = includevalue
  > EOF
  $ cat >> $TESTTMP/simple.rc <<EOF
  > %include $TESTTMP/include.rc
  > EOF
  $ hg config --configfile $TESTTMP/simple.rc includesection
  includesection.includename=includevalue

Order matters
  $ cat >> $TESTTMP/other.rc <<EOF
  > [mysection]
  > myname = othervalue
  > EOF
  $ hg config --configfile $TESTTMP/other.rc --configfile $TESTTMP/simple.rc mysection
  mysection.myname=myvalue
  $ hg config --configfile $TESTTMP/simple.rc --configfile $TESTTMP/other.rc mysection
  mysection.myname=othervalue

Order relative to --config
  $ hg config --configfile $TESTTMP/simple.rc --config mysection.myname=manualvalue mysection
  mysection.myname=manualvalue

Attribution works
  $ hg config --configfile $TESTTMP/simple.rc mysection --debug
  $TESTTMP/simple.rc:2: mysection.myname=myvalue
