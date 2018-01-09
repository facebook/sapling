  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > dialect=
  > show=$TESTDIR/../hgext/fbshow.py
  > EOF

  $ hg help -e show | head -n 1
  show extension - show commits in detail with full log message, patches etc
