#debugruntest-compatible

  $ newrepo
  $ setconfig 'committemplate.changeset={foo}\n'
  $ setconfig 'committemplate.foo=foo'

Default commit template

  $ HGEDITOR=cat hg commit --config ui.allowemptycommit=true
  foo
  abort: commit message unchanged
  [255]

  $ mkdir -p x/y/z/k z/y
  $ touch x/y/z/k/1 x/y/z/1 x/y/1 x/1 z/y/1
  $ echo 'foo = z' > x/y/z/.committemplate
  $ echo 'foo = y' > x/y/.committemplate
  $ echo 'foo = x' > x/.committemplate
  $ echo 'foo = root' > .committemplate

When x/y/z/k/.committemplate does not exist, check parents x/y/z:

  $ hg add -q x/y/z/k/1
  $ HGEDITOR=cat hg commit --config ui.allowemptycommit=true
  z
  abort: commit message unchanged
  [255]

Common prefix is now y:

  $ hg add -q x/y/1
  $ HGEDITOR=cat hg commit --config ui.allowemptycommit=true
  y
  abort: commit message unchanged
  [255]

Common prefix is now repo root:

  $ hg add z/y/1
  $ HGEDITOR=cat hg commit --config ui.allowemptycommit=true
  root
  abort: commit message unchanged
  [255]
