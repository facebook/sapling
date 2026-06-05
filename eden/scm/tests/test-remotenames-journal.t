#chg-compatible

Tests for the journal extension integration with remotenames.

Skip if journal is not available in mercurial

  $ sl help -e journal > /dev/null 2>&1 || exit 80

  $ eagerepo
  $ enable journal

  $ sl init remote
  $ cd remote
  $ touch a
  $ sl commit -A -m 'a commit' -q
  $ sl book bmwillnotmove
  $ sl book bm

Test journal with remote bookmarks works on clone

  $ cd ..
  $ newclientrepo local remote bm bmwillnotmove
  $ sl journal remote/bm
  previous locations of 'remote/bm':
  94cf1ae9e2c8  pull -q -B bm

Test journal with remote bookmarks works on pull

  $ cd ../remote
  $ sl up bm -q
  $ echo 'modified' > a
  $ sl commit -m 'a second commit' -q
  $ cd ../local
  $ sl pull -q
  $ sl journal remote/bm
  previous locations of 'remote/bm':
  b720e98e7160  pull -q
  94cf1ae9e2c8  pull -q -B bm

Test journal with remote bookmarks works after push

  $ sl up remote/bm -q
  $ echo 'modified locally' > a
  $ sl commit -m 'local commit' -q
  $ sl push --to bm -q
  $ sl journal remote/bm
  previous locations of 'remote/bm':
  869ef7e9b417  push --to bm -q
  b720e98e7160  pull -q
  94cf1ae9e2c8  pull -q -B bm

Test second remotebookmark has not been clobbered or has moved since clone

  $ sl journal remote/bmwillnotmove
  previous locations of 'remote/bmwillnotmove':
  94cf1ae9e2c8  pull -q -B bmwillnotmove
