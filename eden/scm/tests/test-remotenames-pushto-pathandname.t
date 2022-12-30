#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ enable remotenames
  $ setconfig remotenames.rename.default=remote remotenames.disallowedto="^remote/"

Init the original "remote" repo

  $ hg init orig
  $ cd orig
  $ echo something > something
  $ hg ci -Am something
  adding something
  $ hg bookmark ababagalamaga
  $ cd ..

Clone original repo

  $ hg clone orig cloned
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd cloned
  $ echo somethingelse > something
  $ hg ci -m somethingelse

Try to push with "remote/"

  $ hg push --to remote/ababagalamaga
  pushing rev 71b4c8f22183 to destination $TESTTMP/orig bookmark ababagalamaga
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark ababagalamaga

Try to push without "remote/", should push to the same bookmark as above

  $ hg push --to ababagalamaga
  pushing rev 71b4c8f22183 to destination $TESTTMP/orig bookmark ababagalamaga
  searching for changes
  remote bookmark already points at pushed rev
  no changes found
  [1]

Set up an svn default push path and test behavior

  $ hg paths --add default-push svn+ssh://nowhere/in/particular
  $ hg push --to foo ../orig
  pushing rev 71b4c8f22183 to destination ../orig bookmark foo
  searching for changes
  abort: not creating new remote bookmark
  (use --create to create a new bookmark)
  [255]
