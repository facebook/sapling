  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > commitextras=$TESTDIR/../hgext3rd/commitextras.py
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
