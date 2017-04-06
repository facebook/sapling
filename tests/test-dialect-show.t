  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > dialect = $TESTDIR/../hgext3rd/dialect.py
  > show = $TESTDIR/../hgext3rd/fbshow.py
  > EOF

  $ hg help -e show | head -n 1
  show extension - show commits in detail with full log message, patches etc
