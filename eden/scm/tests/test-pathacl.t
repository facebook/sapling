#require eden

  $ setconfig diff.git=True
  $ setconfig subtree.min-path-depth=1
  $ setconfig subtree.allow-any-source-commit=True

  $ setconfig pathacl.tent-filter-paths=tent-filter,other-tent-filter

  $ newrepo server
  $ cat > tent-filter << EOF
  > [metadata]
  > title: filter for restricted directories
  > description: This filter defines restricted directories for test
  > version: 2
  > required: true
  > [include]
  > *
  > [exclude]
  > foo/protected
  > EOF
  $ cat > other-tent-filter << EOF
  > [metadata]
  > title: filter for other restricted directories
  > description: This filter defines another restricted directory for test
  > version: 2
  > required: true
  > [include]
  > *
  > [exclude]
  > other/protected
  > EOF
  $ sl ci -Am "add tent-filter files"
  adding other-tent-filter
  adding tent-filter

  $ mkdir -p foo/protected
  $ echo "1\n2\n3\n"> foo/protected/x
  $ echo "a\nb\nc\n" > foo/y
  $ sl ci -Am "add foo"
  adding foo/protected/x
  adding foo/y

  $ mkdir bar
  $ echo "a2\nb\nc\n"> bar/y
  $ sl ci -Am "add bar"
  adding bar/y

  $ echo "11\n2\n3\n"> foo/protected/x
  $ sl ci -m "update foo"

  $ echo "a\nb\nc2\n" > foo/y
  $ sl ci -m "update foo/y"

  $ sl book master

  $ sl log -G -T '{node|short} {desc}\n'
  @  c5213b26753b update foo/y
  │
  o  2889294337a9 update foo
  │
  o  533197f77ed7 add bar
  │
  o  72f85f02032f add foo
  │
  o  71d0e14060d2 add tent-filter files

Setup client repo without enabling tent-filer profile

  $ cd
  $ sl clone -q --eden test:server client1
  $ cd client1

Test copy/move restricted path to outside (should prompt warning and fail by default)

  $ sl cp foo baz
  WARNING: You are attempting to copy restricted data to an unrestricted location:
   * from-path: foo/protected/x (contains restricted data)
   * to-path: baz
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

  $ sl mv foo baz
  WARNING: You are attempting to move restricted data to an unrestricted location:
   * from-path: foo/protected/x (contains restricted data)
   * to-path: baz
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

Test copy/move within restricted path (should succeed)

  $ sl cp foo/protected/x foo/protected/x2
  $ sl st
  A foo/protected/x2
  $ sl go -C . && sl clean
  update complete

  $ sl mv foo/protected/x foo/protected/x2
  $ sl st
  A foo/protected/x2
  R foo/protected/x
  $ sl go -C . && sl clean
  update complete

Test subtree copy restricted path

  $ sl subtree copy --from-path foo --to-path baz
  WARNING: You are attempting to copy restricted data to an unrestricted location:
   * from-path: foo (contains restricted data)
   * to-path: baz
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

  $ sl subtree copy --from-path foo/protected/x --to-path baz/x
  WARNING: You are attempting to copy restricted data to an unrestricted location:
   * from-path: foo/protected/x (contains restricted data)
   * to-path: baz/x
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

Test subtree copy restricted path with absolute path

  $ sl subtree copy --from-path $TESTTMP/client1/foo --to-path $TESTTMP/client1/baz
  WARNING: You are attempting to copy restricted data to an unrestricted location:
   * from-path: foo (contains restricted data)
   * to-path: baz
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

Test subtree copy restricted path in a non-root directory

  $ cd foo
  $ sl subtree copy --from-path ../foo --to-path ../baz
  WARNING: You are attempting to copy restricted data to an unrestricted location:
   * from-path: foo (contains restricted data)
   * to-path: baz
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]
  $ cd ..

Test subtree merge restricted path

  $ sl subtree merge --from-path foo --to-path bar
  WARNING: You are attempting to merge restricted data to an unrestricted location:
   * from-path: foo (contains restricted data)
   * to-path: bar
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

Test subtree merge restricted path with absolute path

  $ sl subtree merge --from-path $TESTTMP/client1/foo --to-path $TESTTMP/client1/bar
  WARNING: You are attempting to merge restricted data to an unrestricted location:
   * from-path: foo (contains restricted data)
   * to-path: bar
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

Test subtree merge restricted path in a non-root directory

  $ cd foo
  $ sl subtree merge --from-path ../foo --to-path ../bar
  WARNING: You are attempting to merge restricted data to an unrestricted location:
   * from-path: foo (contains restricted data)
   * to-path: bar
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

  $ cd ..

Test subtree graft restricted path

  $ sl subtree graft --from-path foo --to-path bar -r 2889294337a9
  WARNING: You are attempting to graft restricted data to an unrestricted location:
   * from-path: foo/protected/x (contains restricted data)
   * to-path: bar
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

Test subtree graft restricted path with absolute path

  $ sl subtree graft --from-path $TESTTMP/client1/foo --to-path $TESTTMP/client1/bar -r 2889294337a9
  WARNING: You are attempting to graft restricted data to an unrestricted location:
   * from-path: foo/protected/x (contains restricted data)
   * to-path: bar
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

Test subtree graft restricted path in a non-root directory

  $ cd foo
  $ sl subtree graft --from-path ../foo --to-path ../bar -r 2889294337a9
  WARNING: You are attempting to graft restricted data to an unrestricted location:
   * from-path: foo/protected/x (contains restricted data)
   * to-path: bar
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]
  $ cd ..

Test subtree copy with addtional filter (sparse profile) path
  $ sl subtree copy --from-path foo --to-path baz --filter tent-filter-not-exist
  abort: path 'tent-filter-not-exist' does not exist in commit c5213b26753b
  [255]
  $ sl subtree copy --from-path foo --to-path baz --filter tent-filter
  copying foo to baz
  $ ls baz
  y

Test subtree copy with a non-exist tent-filter path (the commit does not have the tent-filter)
  $ sl subtree copy --from-path foo --to-path baz2 --config pathacl.tent-filter-paths=tent-filter-not-exist
  copying foo to baz2
  $ ls baz2
  protected
  y

Test copy with disabled other-tent-filter

  $ mkdir -p other/protected
  $ echo "secret" > other/protected/z
  $ sl ci -Am "add other restricted data"
  adding other/protected/z

  $ sl cp other othercopy
  WARNING: You are attempting to copy restricted data to an unrestricted location:
   * from-path: other/protected/z (contains restricted data)
   * to-path: othercopy
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

  $ sl subtree copy --from-path other --to-path othercopy
  WARNING: You are attempting to copy restricted data to an unrestricted location:
   * from-path: other (contains restricted data)
   * to-path: othercopy
  Do you still wish to continue (y/n)?  n
  abort: copying restricted path to an unrestricted path is not allowed
  [255]

Test subtree copy to the restricted directory
  $ sl subtree copy --from-path foo/protected/x --to-path foo/protected/x2
  copying foo/protected/x to foo/protected/x2
  $ ls foo/protected
  x
  x2

Setup client repo with enabling tent-filer profile

  $ cd
  $ sl clone -q --eden test:server client2 --config clone.eden-sparse-filter=tent-filter
  $ cd client2
  $ ls foo
  y

Test subtree copy filters out the restricted paths
  $ sl subtree copy --from-path foo --to-path baz -m "subtree copy foo to baz"
  copying foo to baz
file x should be filtered out
  $ ls baz
  y
  $ sl show
  commit:      6c2778715e38
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       baz/y
  description:
  subtree copy foo to baz
  
  Subtree copy from c5213b26753b88be2e59ce71bdf9dd5f909b0c4d
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

Test subtree merge restricted path with tent-filter enabled
(restricted paths must not leak into the to-side)

  $ sl subtree merge --from-path foo --to-path bar --config subtree.filter-restricted-paths=False
  abort: copying restricted path to an unrestricted path is not allowed
  (WARNING: You are attempting to merge restricted data to an unrestricted location:
   * from-path: foo (contains restricted data)
   * to-path: bar)
  [255]

  $ sl subtree merge --from-path foo --to-path bar
  warning: restricted data was omitted from path 'foo'; result may be incomplete
  searching for merge base ...
  merge base: 72f85f02032f
  merging bar/y and foo/y to bar/y
  1 files merged, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ test ! -e bar/protected || echo BUG: restricted path leaked into bar
  $ sl commit -m "subtree merge foo to bar"

Setup client repo with enabling tent-filer profile for subtree graft

  $ cd
  $ sl clone -q --eden test:server client3 --config clone.eden-sparse-filter=tent-filter
  $ cd client3

Test subtree graft restricted path with tent-filter enabled

  $ sl subtree graft --from-path foo --to-path bar -r 2889294337a9 --config subtree.filter-restricted-paths=False
  abort: copying restricted path to an unrestricted path is not allowed
  (WARNING: You are attempting to graft restricted data to an unrestricted location:
   * from-path: foo/protected/x (contains restricted data)
   * to-path: bar)
  [255]

  $ sl subtree graft --from-path foo --to-path bar -r 2889294337a9
  warning: restricted data was omitted from path 'foo/protected/x'; result may be incomplete
  grafting 2889294337a9 "update foo"
  note: graft of 2889294337a9 created no changes to commit
  $ test ! -e bar/protected || echo BUG: restricted path leaked into bar

Test subtree graft commits that do not have restricted data (should succeed)

  $ sl subtree graft --from-path foo --to-path bar -r c5213b26753b
  grafting c5213b26753b "update foo/y"
  merging bar/y and foo/y to bar/y
