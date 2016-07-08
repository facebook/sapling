  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/commitextras.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > commitextras=$TESTTMP/commitextras.py
  > EOF

Test stuff

  $ hg init repo
  $ cd repo
  $ touch a
  $ hg commit -Aqm a
  $ echo a > a
  $ hg commit -qm a2 --extra oldhash=foo --extra source=bar
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  oldhash=foo
  source=bar
