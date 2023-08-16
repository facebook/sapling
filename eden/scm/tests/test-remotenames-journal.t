#chg-compatible
  $ setconfig experimental.allowfilepeer=True

Tests for the journal extension integration with remotenames.

Skip if journal is not available in mercurial

  $ hg help -e journal &>/dev/null || exit 80

  $ eagerepo
  $ enable journal

  $ hg init remote
  $ cd remote
  $ touch a
  $ hg commit -A -m 'a commit' -q
  $ hg book bmwillnotmove
  $ hg book bm

Test journal with remote bookmarks works on clone

  $ cd ..
  $ newclientrepo local test:remote bm bmwillnotmove
  $ hg journal remote/bm
  previous locations of 'remote/bm':
  94cf1ae9e2c8  pull -q -B bm

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
  94cf1ae9e2c8  pull -q -B bm

Test journal with remote bookmarks works after push

  $ hg up remote/bm -q
  $ echo 'modified locally' > a
  $ hg commit -m 'local commit' -q
  $ hg push --to bm -q
  $ hg journal remote/bm
  previous locations of 'remote/bm':
  869ef7e9b417  push --to bm -q
  b720e98e7160  pull -q
  94cf1ae9e2c8  pull -q -B bm

Test second remotebookmark has not been clobbered or has moved since clone

  $ hg journal remote/bmwillnotmove
  previous locations of 'remote/bmwillnotmove':
  94cf1ae9e2c8  pull -q -B bmwillnotmove

