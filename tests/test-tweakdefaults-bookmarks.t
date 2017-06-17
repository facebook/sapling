
Set up
  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution=all
  > [extensions]
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > tweakdefaults=$TESTDIR/../hgext3rd/tweakdefaults.py
  > EOF

Test hg bookmark works with hidden commits

  $ hg init repo1
  $ cd repo1
  $ touch a
  $ hg commit -A a -m a
  $ echo 1 >> a
  $ hg commit a -m a1
  $ hg prune da7a5140a611 -q
  $ hg bookmark b -r da7a5140a611 -q

Same test but with remotenames enabled

  $ . $TESTDIR/require-ext.sh remotenames
  $ hg bookmark b2 -r da7a5140a611 -q --config extensions.remotenames=
