Make sure the sparse extension does not break functionality when it gets
loaded in a non-sparse repository.

First create a base repository with sparse enabled.

  $ hg init base
  $ cd base
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$TESTDIR/../hgext3rd/sparse.py
  > journal=
  > EOF

  $ echo a > file1
  $ echo x > file2
  $ hg ci -Aqm 'initial'
  $ cd ..

Now create a shared working copy that is not sparse.

  $ hg --config extensions.share= share base shared
  updating working directory
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd shared
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > share=
  > sparse=!
  > journal=
  > EOF

Make sure "hg diff" works in the non-sparse working directory.

  $ echo z >> file1
  $ hg diff |& grep -E 'Unknown exception|AttributeError'
  ** Unknown exception encountered with possibly-broken third-party extension sparse
  AttributeError: 'localrepository' object has no attribute 'sparsematch'
