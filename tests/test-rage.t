  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rage=$TESTDIR/../hgext3rd/rage.py
  > EOF

  $ hg init repo
  $ cd repo
  $ hg rage --preview | grep -o 'blackbox'
  blackbox
