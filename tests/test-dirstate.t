Test dirstate._dirs refcounting:

  $ hg init t
  $ cd t
  $ mkdir -p a/b/c/d
  $ touch a/b/c/d/x
  $ touch a/b/c/d/y
  $ touch a/b/c/d/z
  $ hg ci -Am m
  adding a/b/c/d/x
  adding a/b/c/d/y
  adding a/b/c/d/z
  $ hg mv a z
  moving a/b/c/d/x to z/b/c/d/x
  moving a/b/c/d/y to z/b/c/d/y
  moving a/b/c/d/z to z/b/c/d/z

