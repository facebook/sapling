Source bundle was generated with the following script:

# hg init
# echo a > a
# ln -s a l
# hg ci -Ama -d'0 0'
# mkdir b
# echo a > b/a
# chmod +x b/a
# hg ci -Amb -d'1 0'

  $ hg init
  $ hg -q pull "$TESTDIR/test-manifest.hg"

The next call is expected to return nothing:

  $ hg manifest

  $ hg co
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg manifest
  a
  b/a
  l

  $ hg manifest -v
  644   a
  755 * b/a
  644 @ l

  $ hg manifest --debug
  b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3 644   a
  b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3 755 * b/a
  047b75c6d7a3ef6a2243bd0e99f94f6ea6683597 644 @ l

  $ hg manifest -r 0
  a
  l

  $ hg manifest -r 1
  a
  b/a
  l

  $ hg manifest -r tip
  a
  b/a
  l

  $ hg manifest tip
  a
  b/a
  l


The next two calls are expected to abort:

  $ hg manifest -r 2
  abort: unknown revision '2'!
  [255]

  $ hg manifest -r tip tip
  abort: please specify just one revision
  [255]
