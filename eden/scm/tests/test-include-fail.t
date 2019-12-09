#chg-compatible

#require execbit

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > foo = bar
  > %include $TESTTMP/eperm/rc
  > EOF

  $ mkdir eperm
  $ cat > $TESTTMP/eperm/rc <<EOF
  > [ui]
  > foo = baz
  > EOF

  $ hg config ui.foo
  baz

An EPERM just causes the include to be ignored:

  $ chmod -x eperm
  $ hg config ui.foo
  bar
