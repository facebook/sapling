#require eden

  $ setconfig diff.git=True
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
  > foo/protected
  > EOF
  $ hg ci -Am "add tent-filter file"
  adding tent-filter

  $ mkdir -p foo/protected
  $ echo "1\n2\n3\n"> foo/protected/x
  $ echo "a\nb\nc\n" > foo/y
  $ hg ci -Am "add foo"
  adding foo/protected/x
  adding foo/y

  $ mkdir bar
  $ echo "a2\nb\nc\n"> bar/y
  $ hg ci -Am "add bar"
  adding bar/y

  $ echo "11\n2\n3\n"> foo/protected/x
  $ hg ci -m "update foo"

  $ echo "a\nb\nc2\n" > foo/y
  $ hg ci -m "update foo/y"

  $ hg book master

  $ hg log -G -T '{node|short} {desc}\n'
  @  3dbe1a097d57 update foo/y
  │
  o  bf60887fbaff update foo
  │
  o  6212305f81b9 add bar
  │
  o  3aeb35855961 add foo
  │
  o  5184ab37fc85 add tent-filter file

Setup client repo without enabling tent-filer profile

  $ cd
  $ hg clone -q --eden test:server client1
  $ cd client1

Test copy/move protected path to outside (should prompt warning and fail by default)

  $ hg cp foo baz
  WARNING: You are attempting to copy protected data to an unprotected location:
   * from-path: foo/protected/x (contains protected data)
   * to-path: baz
  Do you still wish to continue (y/n)?  n
  abort: copying protected path to an unprotected path is not allowed
  [255]

  $ hg mv foo baz
  WARNING: You are attempting to move protected data to an unprotected location:
   * from-path: foo/protected/x (contains protected data)
   * to-path: baz
  Do you still wish to continue (y/n)?  n
  abort: copying protected path to an unprotected path is not allowed
  [255]

Test copy/move within protected path (should succeed)

  $ hg cp foo/protected/x foo/protected/x2
  $ hg st
  A foo/protected/x2
  $ hg go -C . && hg clean
  update complete

  $ hg mv foo/protected/x foo/protected/x2
  $ hg st
  A foo/protected/x2
  R foo/protected/x
  $ hg go -C . && hg clean
  update complete

Test subtree copy protected path

  $ hg subtree copy --from-path foo --to-path baz
  WARNING: You are attempting to copy protected data to an unprotected location:
   * from-path: foo (contains protected data)
   * to-path: baz
  Do you still wish to continue (y/n)?  n
  abort: copying protected path to an unprotected path is not allowed
  [255]

  $ hg subtree copy --from-path foo/protected/x --to-path baz/x
  WARNING: You are attempting to copy protected data to an unprotected location:
   * from-path: foo/protected/x (contains protected data)
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

  $ hg subtree graft --from-path foo --to-path bar -r bf60887fbaff
  WARNING: You are attempting to graft protected data to an unprotected location:
   * from-path: foo/protected/x (contains protected data)
   * to-path: bar
  Do you still wish to continue (y/n)?  n
  abort: copying protected path to an unprotected path is not allowed
  [255]

Test subtree copy with addtional filter (sparse profile) path
  $ hg subtree copy --from-path foo --to-path baz --filter tent-filter-not-exist
  abort: path 'tent-filter-not-exist' does not exist in commit 3dbe1a097d57
  [255]
  $ hg subtree copy --from-path foo --to-path baz --filter tent-filter
  copying foo to baz
  $ ls baz
  y

Test subtree copy with a non-exist tent-filter path (the commit does not have the tent-filter)
  $ hg subtree copy --from-path foo --to-path baz2 --config pathacl.tent-filter-path=tent-filter-not-exist
  copying foo to baz2
  $ ls baz2
  protected
  y

Test subtree copy to the protected directory
  $ hg subtree copy --from-path foo/protected/x --to-path foo/protected/x2
  copying foo/protected/x to foo/protected/x2
  $ ls foo/protected
  x
  x2

Setup client repo with enabling tent-filer profile

  $ cd
  $ hg clone -q --eden test:server client2 --config clone.eden-sparse-filter=tent-filter
  $ cd client2
  $ ls foo
  y

Test subtree copy filters out the protected paths
  $ hg subtree copy --from-path foo --to-path baz -m "subtree copy foo to baz"
  copying foo to baz
file x should be filtered out
  $ ls baz
  y
  $ hg show
  commit:      4060440d87ac
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       baz/y
  description:
  subtree copy foo to baz
  
  Subtree copy from 3dbe1a097d576c690e7ef7607cffe27e4681a9b1
  - Copied path foo to baz
  
  
  diff --git a/baz/y b/baz/y
  new file mode 100644
  --- /dev/null
  +++ b/baz/y
  @@ -0,0 +1,4 @@
  +a
  +b
  +c2
  +

Test subtree merge protected path with tent-filter enabled (should fail)

  $ hg subtree merge --from-path foo --to-path bar
  abort: copying protected path to an unprotected path is not allowed
  (WARNING: You are attempting to merge protected data to an unprotected location:
   * from-path: foo (contains protected data)
   * to-path: bar)
  [255]

Test subtree graft protected path with tent-filter enabled (should fail)

  $ hg subtree graft --from-path foo --to-path bar -r bf60887fbaff
  abort: copying protected path to an unprotected path is not allowed
  (WARNING: You are attempting to graft protected data to an unprotected location:
   * from-path: foo/protected/x (contains protected data)
   * to-path: bar)
  [255]

Test subtree graft commits that do not have protected data (should succeed)

  $ hg subtree graft --from-path foo --to-path bar -r 3dbe1a097d57
  grafting 3dbe1a097d57 "update foo/y"
  merging bar/y and foo/y to bar/y
