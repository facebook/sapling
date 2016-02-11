  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rage=$TESTDIR/../rage.py
  > EOF

  $ hg init repo
  $ cd repo
  $ hg rage --preview | grep -o 'blackbox'
  blackbox
