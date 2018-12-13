
Set up
  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution=all
  > [extensions]
  > amend=
  > tweakdefaults=
  > EOF

Test hg bookmark works with hidden commits

  $ hg init repo1
  $ cd repo1
  $ touch a
  $ hg commit -A a -m a
  $ echo 1 >> a
  $ hg commit a -m a1
  $ hg prune da7a5140a611 -q
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg bookmark b -r da7a5140a611 -q

Same test but with remotenames enabled

  $ hg bookmark b2 -r da7a5140a611 -q --config extensions.remotenames=
