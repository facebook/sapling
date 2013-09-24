  $ hg init

  $ echo a > a
  $ hg ci -qAm 'add a'

  $ hg init subrepo
  $ echo 'subrepo = http://example.net/libfoo' > .hgsub
  $ hg ci -qAm 'added subrepo'

  $ hg up -qC 0
  $ echo ax > a
  $ hg ci -m 'changed a'
  created new head

  $ hg up -qC 1
  $ cd subrepo
  $ echo b > b
  $ hg add b
  $ cd ..

Should fail, since there are added files to subrepo:

  $ hg merge
  abort: uncommitted changes in subrepository 'subrepo'
  [255]
