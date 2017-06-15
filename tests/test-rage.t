  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rage=$TESTDIR/../hgext3rd/rage.py
  > EOF

  $ hg init repo
  $ cd repo
#if osx
  $ echo "[rage]" >> .hg/hgrc
  $ echo "rpmbin = /bin/rpm" >> .hg/hgrc
#endif
  $ hg rage --preview | grep -o 'blackbox'
  blackbox
