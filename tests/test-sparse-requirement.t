  $ hg init repo
  $ cd repo

  $ touch a.html b.html c.py d.py

  $ cat > frontend.sparse << EOF
  > [include]
  > *.html
  > EOF

  $ hg -q commit -A -m initial

  $ echo 1 > a.html
  $ echo 1 > c.py
  $ hg commit -m 'commit 1'

Enable sparse profile

  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  revlogv1
  store

  $ hg debugsparse --config extensions.sparse= --enable-profile frontend.sparse
  $ ls
  a.html
  b.html

Requirement for sparse added when sparse is enabled

  $ cat .hg/requires
  dotencode
  exp-sparse
  fncache
  generaldelta
  revlogv1
  store

Client without sparse enabled reacts properly

  $ hg files
  abort: repository is using sparse feature but sparse is not enabled; enable the "sparse" extensions to access!
  [255]

Requirement for sparse is removed when sparse is disabled

  $ hg debugsparse --reset --config extensions.sparse=

  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  revlogv1
  store

And client without sparse can access

  $ hg files
  a.html
  b.html
  c.py
  d.py
  frontend.sparse
