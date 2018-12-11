  $ . $TESTDIR/library.sh

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
  running *scm/mononoke/tests/integration/dummyssh.par 'user@dummy' ''\''*scm/mononoke/hgcli/hgcli#binary/hgcli'\'' -R repo serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  remote: * (glob)
  remote: capabilities: lookup known getbundle unbundle=HG10GZ,HG10BZ,HG10UN gettreepack remotefilelog pushkey stream-preferred stream_option streamreqs=generaldelta,lz4revlog,revlogv1 treeonly bundle2=* (glob)
  remote: 1
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
  remote: * ERRO Command failed, remote: true, error: Repo is marked as read-only, root_cause: RepoReadOnly, backtrace: , session_uuid: * (glob)
  abort: unexpected response: empty string
  [255]
