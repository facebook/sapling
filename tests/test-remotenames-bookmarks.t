  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > remotenames=
  > EOF

Setup repo

  $ hg init repo
  $ cd repo
  $ echo 'foo' > a.txt
  $ hg add a.txt
  $ hg commit -m 'a'

Testing bookmark options without args
  $ hg bookmark a
  $ hg bookmark b
  $ hg bookmark -v
     a                         0:2dcb9139ea49
   * b                         0:2dcb9139ea49
  $ hg bookmark --track a
  $ hg bookmark -v
     a                         0:2dcb9139ea49
   * b                         0:2dcb9139ea49            [a]
  $ hg bookmark --untrack
  $ hg bookmark -v
     a                         0:2dcb9139ea49
   * b                         0:2dcb9139ea49
