
#require no-eden

  $ eagerepo
  $ configure dummyssh modernclient
  $ newclientrepo repo

Empty
  $ hg log --configfile
  hg log: option --configfile requires argument
  (use 'hg log -h' to get help)
  [255]

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

Cloning adds --configfile values to .hg/hgrc
  $ cd ..
  $ hg clone -q test:repo_server repo2 --configfile $TESTTMP/simple.rc --configfile $TESTTMP/other.rc
  $ dos2unix repo2/.hg/hgrc
  %include $TESTTMP/simple.rc
  %include $TESTTMP/other.rc
  
  [paths]
  default = test:repo_server
