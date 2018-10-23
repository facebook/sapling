  $ . $TESTDIR/library.sh

  $ hook_test_setup $TESTDIR/hooks/deny_files.lua deny_files PerAddedOrModifiedFile

Negative testing
  $ hg up -q 0
  $ echo "good" > good_file.txt
  $ hg ci -Aqm negative
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 7de92e406b02 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update
  remote: * DEBG Session with Mononoke started with uuid: * (glob)

Tricky case - this should succeed, but looks very similar to cases that should not
  $ hg up -q 0
  $ mkdir -p test-buck-out/buck-out-test/
  $ echo "good" > test-buck-out/buck-out-test/buck-out
  $ hg ci -Aqm negative
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 94d93052245d to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

buck-out directory is blacklisted in the root
  $ hg up -q 0
  $ mkdir -p buck-out/
  $ echo "bad" > buck-out/file
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev f8301844633b to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: deny_files for f8301844633b60c2b4f8b990279394d831ab90c7: Denied filename 'buck-out/file' matched name pattern '^buck%-out/'. Rename or remove this file and try again., root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\ndeny_files for f8301844633b60c2b4f8b990279394d831ab90c7: Denied filename \'buck-out/file\' matched name pattern \'^buck%-out/\'. Rename or remove this file and try again."
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

buck-out directory is blacklisted in any subdir
  $ hg up -q 0
  $ mkdir -p dir/buck-out
  $ echo "bad" > dir/buck-out/file
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 409273951981 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: deny_files for 40927395198136c0dc65978d4fec6a8bf8386d4d: Denied filename 'dir/buck-out/file' matched name pattern '/buck%-out/'. Rename or remove this file and try again., root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\ndeny_files for 40927395198136c0dc65978d4fec6a8bf8386d4d: Denied filename \'dir/buck-out/file\' matched name pattern \'/buck%-out/\'. Rename or remove this file and try again."
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED does the needful
  $ hg up -q 0
  $ echo "bad" > important_DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED.txt
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev d1a6e60539c6 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: deny_files for d1a6e60539c6d4cd8df0c1fd442dcba98ef76bdf: Denied filename 'important_DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED.txt' matched name pattern 'DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED'. Rename or remove this file and try again., root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\ndeny_files for d1a6e60539c6d4cd8df0c1fd442dcba98ef76bdf: Denied filename \'important_DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED.txt\' matched name pattern \'DO_NOT_COMMIT_THIS_FILE_OR_YOU_WILL_BE_FIRED\'. Rename or remove this file and try again."
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Old fbmake leftovers cannot be committed
  $ hg up -q 0
  $ mkdir -p fbcode/_bin
  $ echo "bad" > fbcode/_bin/file
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 10b8f7a92bd1 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: deny_files for 10b8f7a92bd16630481eac34cac5b832edb9cb71: Denied filename 'fbcode/_bin/file' matched name pattern '^fbcode/_bin/'. Rename or remove this file and try again., root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\ndeny_files for 10b8f7a92bd16630481eac34cac5b832edb9cb71: Denied filename \'fbcode/_bin/file\' matched name pattern \'^fbcode/_bin/\'. Rename or remove this file and try again."
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Cannot nest project dirs badly
  $ hg up -q 0
  $ for path in fbandroid/fbandroid fbcode/fbcode fbobjc/fbobjc xplat/xplat; do
  > mkdir -p $path
  > echo fail > $path/files
  > done
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 5d971d690977 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: deny_files for 5d971d690977075710cf1270860e2ab65015eeec: Denied filename 'fbandroid/fbandroid/files' matched name pattern '^fbandroid/fbandroid/'. Rename or remove this file and try again.
  remote: deny_files for 5d971d690977075710cf1270860e2ab65015eeec: Denied filename 'fbcode/fbcode/files' matched name pattern '^fbcode/fbcode/'. Rename or remove this file and try again.
  remote: deny_files for 5d971d690977075710cf1270860e2ab65015eeec: Denied filename 'fbobjc/fbobjc/files' matched name pattern '^fbobjc/fbobjc/'. Rename or remove this file and try again.
  remote: deny_files for 5d971d690977075710cf1270860e2ab65015eeec: Denied filename 'xplat/xplat/files' matched name pattern '^xplat/xplat/'. Rename or remove this file and try again., root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\ndeny_files for 5d971d690977075710cf1270860e2ab65015eeec: Denied filename \'fbandroid/fbandroid/files\' matched name pattern \'^fbandroid/fbandroid/\'. Rename or remove this file and try again.\ndeny_files for 5d971d690977075710cf1270860e2ab65015eeec: Denied filename \'fbcode/fbcode/files\' matched name pattern \'^fbcode/fbcode/\'. Rename or remove this file and try again.\ndeny_files for 5d971d690977075710cf1270860e2ab65015eeec: Denied filename \'fbobjc/fbobjc/files\' matched name pattern \'^fbobjc/fbobjc/\'. Rename or remove this file and try again.\ndeny_files for 5d971d690977075710cf1270860e2ab65015eeec: Denied filename \'xplat/xplat/files\' matched name pattern \'^xplat/xplat/\'. Rename or remove this file and try again."
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Cannot put crud in xplat
  $ hg up -q 0
  $ for path in xplat/fbandroid xplat/fbcode xplat/fbobjc; do
  > mkdir -p $path
  > echo fail > $path/files
  > done
  $ hg ci -Aqm failure
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 42bbe801bb55 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: deny_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Denied filename 'xplat/fbandroid/files' matched name pattern '^xplat/fbandroid/'. Rename or remove this file and try again.
  remote: deny_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Denied filename 'xplat/fbcode/files' matched name pattern '^xplat/fbcode/'. Rename or remove this file and try again.
  remote: deny_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Denied filename 'xplat/fbobjc/files' matched name pattern '^xplat/fbobjc/'. Rename or remove this file and try again., root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\ndeny_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Denied filename \'xplat/fbandroid/files\' matched name pattern \'^xplat/fbandroid/\'. Rename or remove this file and try again.\ndeny_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Denied filename \'xplat/fbcode/files\' matched name pattern \'^xplat/fbcode/\'. Rename or remove this file and try again.\ndeny_files for 42bbe801bb55fd1eee91d4dbb56f5a5dc3f0f0ad: Denied filename \'xplat/fbobjc/files\' matched name pattern \'^xplat/fbobjc/\'. Rename or remove this file and try again."
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
