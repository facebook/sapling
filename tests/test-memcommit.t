  $ setupserver()
  > {
  >   hg init "$1"
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
  >   copycommit "$@"
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


- Test that we cannot make a commit without specifying a parent in the default
configuration.

  $ cd originalrepo
  $ touch x
  $ hg commit -Aqm "initial commit"
  $ copycommit . ../mirroredrepo
  abort: commit without parents are not supported
  [255]


- Test that we can make a commit without specifying a parent in the
memcommit.allowdetachedheads=true configuration.

  $ hg debugserializecommit | \
  > hg -R ../mirroredrepo --config memcommit.allowunrelatedroots=true memcommit
  $ testreposimilarity . ../mirroredrepo


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
  abort: destination parent does not match destination bookmark
  [255]


- Test that destination bookmark is required in case of pushrebase.

  $ copycommit . ../mirroredrepo --pushrebase
  abort: must specify destination bookmark for pushrebase
  [255]


- Test that the conflicts are reported as expected in case of pushrebase.

  $ copycommit . ../mirroredrepo -d master --pushrebase
  abort: conflicting changes in:
      test
  (pull and rebase your changes locally, then try again)
  [255]

  $ testmemcommit . ../mirroredrepo


- Setup client for pushrebase tests.

  $ setupclient()
  > {
  >   hg init "$1"
  >   ( \
  >     cd "$1" && enable memcommit pushrebase remotenames && \
  >     setconfig experimental.evolution= && \
  >     setconfig ui.ssh="python \"$TESTDIR/dummyssh\""
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
  $ copycommit . ../mirroredrepo -d master --pushrebase
  $ hg push -q ssh://user@dummy/originalrepo --to master
  $ testreposimilarity ../originalrepo ../mirroredrepo


- Test that the pushrebase succeeds when the commit parent is a descendant of
destination bookmark.

  $ cd ../originalrepo
  $ hg -q up "master"
  $ touch test3
  $ hg commit -Aqm "file without content"
  $ copycommit . ../mirroredrepo
  $ hg mv test3 test4
  $ hg commit -qm "renamed file"
  $ copycommit . ../mirroredrepo

  $ cd ../client
  $ hg -q pull ssh://user@dummy/originalrepo
  $ hg -q up "tip"
  $ hg rm test4
  $ hg commit -qm "deleted file"
  $ copycommit . ../mirroredrepo -d master --pushrebase
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
  abort: destination bookmark is not ancestor or descendant of commit parent
  [255]


- Test that we fail to create merge commits.

  $ cd ../originalrepo
  $ hg -q merge "master"
  $ hg commit -qm "merge commit"
  $ copycommit . ../mirroredrepo
  abort: merge commits are not supported
  [255]
