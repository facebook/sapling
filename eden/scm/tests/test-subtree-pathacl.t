#require eden

  $ setconfig subtree.min-path-depth=1
  $ setconfig subtree.allow-any-source-commit=True

  $ setconfig pathacl.tent-filter-path=tent-filter

  $ newrepo server
  $ cat > tent-filter << EOF
  > [metadata]
  > title: filter for protected directories
  > description: This filter defines protected directories for test
  > version: 2
  > required: true
  > [include]
  > *
  > [exclude]
  > foo/x
  > EOF
  $ hg ci -Am "add tent-filter file"
  adding tent-filter

  $ mkdir foo
  $ echo "1\n2\n3\n"> foo/x
  $ hg ci -Am "add foo"
  adding foo/x

  $ mkdir bar
  $ echo "1\n2\n3\n"> bar/x
  $ hg ci -Am "add bar"
  adding bar/x

  $ echo "11\n2\n3\n"> foo/x
  $ hg ci -m "update foo"

  $ hg book master

  $ hg log -G -T '{node|short} {desc}\n'
  @  6ccc1aafdbcb update foo
  │
  o  1e48bf7882cb add bar
  │
  o  efc0acd15b30 add foo
  │
  o  183a8fb76979 add tent-filter file

Setup client repo without enabling tent-filer profile

  $ cd
  $ hg clone -q --eden test:server client1
  $ cd client1

Test subtree copy protected path

  $ hg subtree copy --from-path foo --to-path baz
  WARNING: You are attempting to copy protected data to an unprotected location:
   * from-path: foo (contains protected data)
   * to-path: baz
  Do you still wish to continue (y/n)?  n
  abort: copying protected path to an unprotected path is not allowed
  [255]

  $ hg subtree copy --from-path foo/x --to-path baz/x
  WARNING: You are attempting to copy protected data to an unprotected location:
   * from-path: foo/x (contains protected data)
   * to-path: baz/x
  Do you still wish to continue (y/n)?  n
  abort: copying protected path to an unprotected path is not allowed
  [255]

Test subtree merge protected path

  $ hg subtree merge --from-path foo --to-path bar
  WARNING: You are attempting to merge protected data to an unprotected location:
   * from-path: foo (contains protected data)
   * to-path: bar
  Do you still wish to continue (y/n)?  n
  abort: copying protected path to an unprotected path is not allowed
  [255]

Test subtree graft protected path

  $ hg subtree graft --from-path foo --to-path bar -r 6ccc1aafdbcb
  WARNING: You are attempting to graft protected data to an unprotected location:
   * from-path: foo (contains protected data)
   * to-path: bar
  Do you still wish to continue (y/n)?  n
  abort: copying protected path to an unprotected path is not allowed
  [255]
