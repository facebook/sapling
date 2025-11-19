  $ setconfig subtree.min-path-depth=1
  $ setconfig subtree.allow-any-source-commit=True

  $ setconfig pathacl.tent-filter-path=tent-filter

  $ newclientrepo
  $ cat > tent-filter << EOF
  > [metadata]
  > title: filter for protected directories
  > description: This filter defines protected directories for test
  > version: 2
  > required: true
  > [include]
  > *
  > [exclude]
  > foo
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

  $ hg log -G -T '{node|short} {desc}\n'
  @  6d341104cfe6 update foo
  │
  o  581863034e2e add bar
  │
  o  e33a34058170 add foo
  │
  o  e470fb7efd32 add tent-filter file

Test subtree copy protected path

  $ hg subtree copy --from-path foo --to-path baz
  WARNING: You are attempting to copy protected data to an unprotected location:
   * from-path: foo (contains protected data)
   * to-path: baz
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
