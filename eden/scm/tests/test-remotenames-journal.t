#chg-compatible

  $ setconfig extensions.treemanifest=!
Tests for the journal extension integration with remotenames.

Skip if journal is not available in mercurial

  $ hg help -e journal &>/dev/null || exit 80

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > journal=
  > remotenames=
  > [remotenames]
  > rename.default=remote
  > EOF

  $ hg init remote
  $ cd remote
  $ touch a
  $ hg commit -A -m 'a commit' -q
  $ hg book bmwillnotmove
  $ hg book bm

Test journal with remote bookmarks works on clone

  $ cd ..
  $ hg clone remote local -q
  $ cd local
  $ hg journal remote/bm
  previous locations of 'remote/bm':
  94cf1ae9e2c8  clone remote local -q

Test journal with remote bookmarks works on pull

  $ cd ../remote
  $ hg up bm -q
  $ echo 'modified' > a
  $ hg commit -m 'a second commit' -q
  $ cd ../local
  $ hg pull -q
  $ hg journal remote/bm
  previous locations of 'remote/bm':
  b720e98e7160  pull -q
  94cf1ae9e2c8  clone remote local -q

Test journal with remote bookmarks works after push

  $ hg up remote/bm -q
  $ echo 'modified locally' > a
  $ hg commit -m 'local commit' -q
  $ hg push --to bm -q
  $ hg journal remote/bm
  previous locations of 'remote/bm':
  869ef7e9b417  push --to bm -q
  b720e98e7160  pull -q
  94cf1ae9e2c8  clone remote local -q

Test second remotebookmark has not been clobbered or has moved since clone

  $ hg journal remote/bmwillnotmove
  previous locations of 'remote/bmwillnotmove':
  94cf1ae9e2c8  clone remote local -q

