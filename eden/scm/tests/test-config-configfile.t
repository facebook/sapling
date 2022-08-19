#chg-compatible
#debugruntest-compatible
  $ configure dummyssh modernclient
  $ hg init repo
  $ cd repo

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
  $TESTTMP/simple.rc: mysection.myname=myvalue

Cloning adds --configfile values to .hg/hgrc
  $ cd ..
  $ hg clone ssh://user@dummy/repo repo2 --configfile $TESTTMP/simple.rc --configfile $TESTTMP/other.rc
  no changes found
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ dos2unix repo2/.hg/hgrc
  # example repository config (see 'hg help config' for more info)
  [paths]
  default = ssh://user@dummy/repo
  
  # path aliases to other clones of this repo in URLs or filesystem paths
  # (see 'hg help config.paths' for more info)
  #
  # default:pushurl = ssh://jdoe@example.net/hg/jdoes-fork
  # my-fork         = ssh://jdoe@example.net/hg/jdoes-fork
  # my-clone        = /home/jdoe/jdoes-clone
  
  [ui]
  # name and email (local to this repository, optional), e.g.
  # username = Jane Doe <jdoe@example.com>
  
  %include $TESTTMP/simple.rc
  %include $TESTTMP/other.rc
