  $ . $TESTDIR/library.sh

  $ hook_test_setup $TESTDIR/hooks/gitattributes-textdirectives.lua gitattributes-textdirectives PerAddedOrModifiedFile "bypass_commit_string=\"@allow-gitattributes-textdirectives\""

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

add non-gitattributes file that matches regex - should pass

  $ echo 'text=auto' > not-gitattributes
  $ hg add -q not-gitattributes
  $ hg ci -qm 'not a .gitattributes file'
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev cf5d1795c7a4 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

add .gitattributes file that matches regex - should fail

  $ echo 'text=auto' > .gitattributes
  $ hg add -q .gitattributes
  $ hg ci -qm '.gitattributes file with illegal content'
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 15bff1dfbf94 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: gitattributes-textdirectives for 15bff1dfbf942b0e37fedd9cb530b11bdf30a636: No text directives are authorized in .gitattributes. This is known to break sandcastle and developers' local clones., root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\ngitattributes-textdirectives for 15bff1dfbf942b0e37fedd9cb530b11bdf30a636: No text directives are authorized in .gitattributes. This is known to break sandcastle and developers\' local clones."
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

make sure . in "%.gitattributes$" regex is a literal dot and not a wildcard - should pass

  $ hg mv -q .gitattributes agitattributes
  $ hg ci --amend -qm 'agitattributes can have illegal .gitattributes pattern'
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 7c50ba5a883b to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

.gitattributes file without the illegal pattern - should pass

  $ echo 'ext=auto' > .gitattributes
  $ hg add -q .gitattributes
  $ hg ci -qm '.gitattributes file with pattern that should not match regex'
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev c73775b931dc to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

.gitattributes file with the illegal pattern (exercising wildcards) - should fail
(It's certainly possible that our regex is too loose.)

  $ echo 'look! text xxx = xxx auto' > foo.gitattributes
  $ hg add -q foo.gitattributes
  $ hg ci -qm '*.gitattributes file with illegal content'
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 952b5a8c12fa to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: gitattributes-textdirectives for 952b5a8c12fa9aa4d78e6deed0594ac4fb6cd8a4: No text directives are authorized in .gitattributes. This is known to break sandcastle and developers' local clones., root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\ngitattributes-textdirectives for 952b5a8c12fa9aa4d78e6deed0594ac4fb6cd8a4: No text directives are authorized in .gitattributes. This is known to break sandcastle and developers\' local clones."
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
