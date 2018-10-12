  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > dialect=
  > show=
  > EOF

  $ hg help -e show | head -n 1
  show extension - show commits in detail with full log message, patches etc
