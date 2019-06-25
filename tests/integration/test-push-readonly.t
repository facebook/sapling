  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export READ_ONLY_REPO=1
  $ setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup master bookmarks

  $ hg bookmark master_bookmark -r 'tip'

verify content
  $ hg log
  changeset:   0:0e7ec5675652
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

setup push source repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2

start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

create new commit in repo2 and check that push fails

  $ cd repo2
  $ mkdir b_dir
  $ echo "new a file content" > a
  $ echo "b file content" > b_dir/b
  $ hg add b_dir/b
  $ hg ci -mb

  $ hgmn push --force --config treemanifest.treeonly=True --debug ssh://user@dummy/repo
  pushing to ssh://user@dummy/repo
  running * 'user@dummy' '$TESTTMP/mononoke_hgcli -R repo serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  1 changesets found
  list of changesets:
  bb0985934a0f8a493887892173b68940ceb40b4f
  sending unbundle command
  bundle2-output-bundle: "HG20", 3 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  remote: Command failed
  remote:   Error:
  remote:     Repo is marked as read-only: Set by config option
  remote:   Root cause:
  remote:     RepoReadOnly(
  remote:         "Set by config option",
  remote:     )
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Try to bypass the check
  $ hgmn push --force --config treemanifest.treeonly=True ssh://user@dummy/repo --pushvars "BYPASS_READONLY=true"
  pushing to ssh://user@dummy/repo
  searching for changes
