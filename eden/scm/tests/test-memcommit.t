#chg-compatible

#chg-compatible

#testcases hgsql.true hgsql.false

  $ setconfig extensions.treemanifest=!
  $ . "$TESTDIR/hgsql/library.sh"

#if hgsql.false
  $ initserver()
  > {
  >   hg init "$1"
  > }
#endif

  $ setupserver()
  > {
  >   initserver "$1" "$1"
  >   ( cd "$1" && enable commitextras memcommit pushrebase )
  > }

  $ setupserver originalrepo
  $ setupserver mirroredrepo

  $ copycommit()
  > {
  >   originalrepo="$1"
  >   shift
  >   mirroredrepo="$1"
  >   shift
  >   hg -R "${originalrepo}" debugserializecommit "$@" | \
  >   hg -R "${mirroredrepo}" memcommit
  > }

  $ testreposimilarity()
  > {
  >   diff <(hg -R "$1" heads) <(hg -R "$2" heads)
  > }

  $ testmemcommit()
  > {
  >   copycommit "$@" > /dev/null
  >   testreposimilarity "$1" "$2"
  > }

  $ testcommits()
  > {
  >   chmod +x test
  >   hg commit -qm "set executable flag for test"
  >   testmemcommit "$@"
  >   hg cp test test1
  >   hg commit -qm "copied test->test1"
  >   testmemcommit "$@"
  >   ln -s test1 test2
  >   hg commit -Aqm "create soft link test2 for test1"
  >   testmemcommit "$@"
  >   hg rm test1 test2
  >   hg commit -qm "deleted test1 and test2"
  >   testmemcommit "$@"
  >   hg mv test test1
  >   hg commit -qm "renamed test->test1 with commit extra" --extra "key=value"
  >   testmemcommit "$@"
  >   hg rm test1
  >   hg commit -qm "deleted test1 with commit extra" --extra "key1=value1"
  >   testmemcommit "$@"
  > }

  $ testmirroring()
  > {
  >   touch test
  >   hg commit -Aqm "added test without content"
  >   testmemcommit "$@"
  >   testcommits "$@"
  >   echo "a" >> test
  >   hg commit -Aqm "added test with content"
  >   testmemcommit "$@"
  >   testcommits "$@"
  >   hg bundle -q --base -2 test
  >   hg commit -Aqm "added test with binary content"
  >   testmemcommit "$@"
  >   testcommits "$@"
  > }


- Test that the `-q` results in no output.

  $ cd originalrepo
  $ hg -q memcommit
  [255]


- Test that we cannot make a commit without specifying a parent in the default
configuration.

  $ touch x
  $ hg commit -Aqm "initial commit"
  $ copycommit . ../mirroredrepo
  {"error": "commit without parents are not allowed"} (no-eol)
  [255]


- Test that we can make a commit without specifying a parent in the
memcommit.allowunrelatedroots=true configuration. Also, test that we get the new
commit hash in the output.

  $ hg debugserializecommit | \
  > hg -R ../mirroredrepo --config memcommit.allowunrelatedroots=true memcommit
  {"hash": "eae37600e40b803aa5f53aa9dbf9c45eae74323c"} (no-eol)
  $ testreposimilarity . ../mirroredrepo


- Test that the '--to' option works.

  $ echo >> y
  $ hg commit -Aqm "added another file"
  $ testmemcommit . ../mirroredrepo --to ".^"


- Test that committing to new parents is not supported.

  $ copycommit . ../mirroredrepo --to "."
  {"error": "commit with new parents not supported"} (no-eol)
  [255]


- Test that we can mirror commits from the originalrepo to the mirroredrepo. In
this case, each commit will be created on the parent commit specified in the
memcommit request.

  $ testmirroring . ../mirroredrepo


- Test that we can mirror commits from the originalrepo to the mirroredrepo when
the destination bookmark is specified. In this case, each commit will be created
on the destination bookmark specified in the memcommit request.

  $ cd ../mirroredrepo
  $ hg bookmark -r "tip" master

  $ cd ../originalrepo
  $ hg bookmark -r "tip" master

Make the master bookmark active.
  $ hg -q up master

  $ testmirroring . ../mirroredrepo -d "master"


- For now, we require that the destination bookmark is the same as the commit
parent when the destination bookmark is specified. Confirm that we fail if that
is not the case.

  $ hg -q up ".~2"
  $ hg mv test test2
  $ hg commit -qm "renamed file"

  $ copycommit . ../mirroredrepo -d master
  {"error": "destination parent does not match destination bookmark"} (no-eol)
  [255]


- Test that destination bookmark is required in case of pushrebase.

  $ copycommit . ../mirroredrepo --pushrebase
  {"error": "must specify destination bookmark for pushrebase"} (no-eol)
  [255]


- Test that the conflicts are reported as expected in case of pushrebase.

  $ copycommit . ../mirroredrepo -d master --pushrebase
  {"error": "conflicting changes in:\n    test"} (no-eol)
  [255]

  $ testmemcommit . ../mirroredrepo


- Setup client for pushrebase tests.

#if hgsql.false
  $ initclient()
  > {
  >   hg init "$1"
  >   ( cd "$1" && setconfig ui.ssh="python \"$TESTDIR/dummyssh\"" )
  > }
#endif

  $ setupclient()
  > {
  >   initclient "$1"
  >   ( \
  >     cd "$1" && enable memcommit pushrebase remotenames && \
  >     setconfig experimental.evolution=
  >   )
  > }

  $ cd ..
  $ setupclient client
  $ cd client
  $ hg -q pull ssh://user@dummy/originalrepo


- Test that the pushrebase succeeds when the commit parent is an ancestor of
destination bookmark.

  $ hg -q up "tip^"
  $ touch test2
  $ hg commit -Aqm "file without content"
  $ copycommit . ../mirroredrepo -d master --pushrebase > /dev/null
  $ hg push -q ssh://user@dummy/originalrepo --to master
  $ testreposimilarity ../originalrepo ../mirroredrepo


- Test that the pushrebase succeeds when the commit parent is a descendant of
destination bookmark.

  $ cd ../originalrepo
  $ hg -q up "master"
  $ touch test3
  $ hg commit -Aqm "file without content"
  $ copycommit . ../mirroredrepo > /dev/null
  $ hg mv test3 test4
  $ hg commit -qm "renamed file"
  $ copycommit . ../mirroredrepo > /dev/null

  $ cd ../client
  $ hg -q pull ssh://user@dummy/originalrepo
  $ hg -q up "tip"
  $ hg rm test4
  $ hg commit -qm "deleted file"
  $ copycommit . ../mirroredrepo -d master --pushrebase > /dev/null
  $ hg -q push ssh://user@dummy/originalrepo --to master
  $ testreposimilarity ../originalrepo ../mirroredrepo


- Test that the pushrebase fails when the commit parent is neither ancestor nor
descendent of destination bookmark.

  $ cd ../originalrepo
  $ hg -q up "master^"
  $ touch test5
  $ hg commit -Aqm "file without content"
  $ copycommit . ../mirroredrepo > /dev/null

  $ cd ../client
  $ hg -q pull ssh://user@dummy/originalrepo
  $ hg -q up "tip"
  $ hg rm test5
  $ hg commit -qm "deleted file"
  $ copycommit . ../mirroredrepo -d master --pushrebase
  {"error": "destination bookmark is not ancestor or descendant of commit parent"} (no-eol)
  [255]


- Test that we fail to create merge commits.

  $ cd ../originalrepo
  $ copycommit . ../mirroredrepo --to ". + .^"
  {"error": "merge commits are not supported"} (no-eol)
  [255]

  $ hg -q merge "master"
  $ hg commit -qm "merge commit"
  $ copycommit . ../mirroredrepo
  {"error": "merge commits are not supported"} (no-eol)
  [255]
