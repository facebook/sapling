#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ disable treemanifest
  $ configure dummyssh
#require serve

  $ hg init test
  $ cd test

  $ echo foo>foo
  $ hg addremove
  adding foo
  $ hg commit -m 1

  $ hg verify
  warning: verify does not actually check anything in this repo

  $ cd ..

  $ hg clone --pull ssh://user@dummy/test copy
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd copy
  $ hg verify
  warning: verify does not actually check anything in this repo

  $ hg co
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat foo
  foo

  $ hg manifest --debug
  2ed2a3912a0b24502043eae84ee4b279c18b90dd 644   foo

  $ hg pull
  pulling from ssh://user@dummy/test (glob)
  searching for changes
  no changes found

Test pull of non-existing 20 character revision specification, making sure plain ascii identifiers
not are encoded like a node:

  $ hg pull -r 'xxxxxxxxxxxxxxxxxxxy'
  pulling from ssh://user@dummy/test (glob)
  abort: unknown revision 'xxxxxxxxxxxxxxxxxxxy'!
  [255]
  $ hg pull -r 'xxxxxxxxxxxxxxxxxx y'
  pulling from ssh://user@dummy/test (glob)
  abort: unknown revision '7878787878787878787878787878787878782079'! (py2 !)
  abort: unknown revision 'xxxxxxxxxxxxxxxxxx y'! (no-py2 !)
  [255]

Issue622: hg init && hg pull -u URL doesn't checkout default branch

  $ cd ..
  $ hg init empty
  $ cd empty
  $ hg pull -u ../test
  pulling from ../test
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test 'file:' uri handling:

  $ hg pull -q file://../test-does-not-exist
  abort: file:// URLs can only refer to localhost
  [255]

  $ hg pull -q file://../test
  abort: file:// URLs can only refer to localhost
  [255]

MSYS changes 'file:' into 'file;'

#if no-msys
  $ hg pull -q file:../test  # no-msys
#endif

It's tricky to make file:// URLs working on every platform with
regular shell commands.

  $ URL=`hg debugshell -c "import os; ui.write('file://foobar' + ('/' + os.getcwd().replace(os.sep, '/')).replace('//', '/') + '/../test')"`
  $ hg pull -q "$URL"
  abort: file:// URLs can only refer to localhost
  [255]

  $ URL=`hg debugshell -c "import os; ui.write('file://localhost' + ('/' + os.getcwd().replace(os.sep, '/')).replace('//', '/') + '/../test')"`
  $ hg pull -q "$URL"

