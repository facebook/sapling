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
  $ echo "y" > foo/y
  $ hg ci -Am "add foo"
  adding foo/x
  adding foo/y

  $ mkdir bar
  $ echo "1\n2\n3\n"> bar/x
  $ hg ci -Am "add bar"
  adding bar/x

  $ echo "11\n2\n3\n"> foo/x
  $ hg ci -m "update foo"

  $ hg book master

  $ hg log -G -T '{node|short} {desc}\n'
  @  bdcb96c08db3 update foo
  │
  o  828d81ffd0aa add bar
  │
  o  8d49f4ffde71 add foo
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

  $ hg subtree graft --from-path foo --to-path bar -r bdcb96c08db3
  WARNING: You are attempting to graft protected data to an unprotected location:
   * from-path: foo (contains protected data)
   * to-path: bar
  Do you still wish to continue (y/n)?  n
  abort: copying protected path to an unprotected path is not allowed
  [255]

Setup client repo with enabling tent-filer profile

  $ cd
  $ hg clone -q --eden test:server client2 --config clone.eden-sparse-filter=tent-filter
  $ cd client2
  $ ls foo
  y

Test subtree copy filters out the protected paths
  $ hg subtree copy --from-path foo --to-path baz -m "subtree copy foo to baz"
  copying foo to baz
tofix: x should be filtered out
  $ ls baz
  x
  y
