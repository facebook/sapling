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

Test with shared repo
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > share=
  > EOF
  $ cd ..
  $ hg share repo repo2
  updating working directory
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Create fake infinitepush backup state to be collected by rage

  $ echo "fakestate" > repo/.hg/infinitepushbackupstate
  $ cd repo2
  $ hg rage --preview | grep -o 'fakestate'
  fakestate

