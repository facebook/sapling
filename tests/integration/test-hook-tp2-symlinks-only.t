  $ . $TESTDIR/library.sh

  $ hook_test_setup $TESTDIR/hooks/tp2_symlinks_only.lua tp2_symlinks_only PerAddedOrModifiedFile "bypass_commit_string=\"@allow-non-symlink-tp2\""

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

add symlink - should pass

  $ mkdir -p fbcode/third-party2
  $ mkdir otherdir
  $ echo 'x' > otherdir/foo
  $ ln -s otherdir/foo fbcode/third-party2/some-link
  $ hg add -q otherdir
  $ hg add -q fbcode/third-party2
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 5cd3cebef181 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

add non symlink - should fail

  $ echo 'x' > fbcode/third-party2/non-link
  $ hg add fbcode/third-party2/non-link
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 566e4f57650f to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: tp2_symlinks_only for 566e4f57650f7358c94e7ed85e661957343ca6f7: All projects committed to fbcode/third-party2/ must be symlinks, root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\ntp2_symlinks_only for 566e4f57650f7358c94e7ed85e661957343ca6f7: All projects committed to fbcode/third-party2/ must be symlinks"
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

with override - should pass

  $ hg up -q 5cd3cebef181
  $ echo 'x' > fbcode/third-party2/non-link
  $ hg add fbcode/third-party2/non-link
  $ hg ci -Aqm "@allow-non-symlink-tp2"
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 86ac215a2c47 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update
