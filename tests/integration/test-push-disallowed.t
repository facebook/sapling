  $ . $TESTDIR/library.sh

setup configuration

  $ DISALLOW_NON_PUSHREBASE=1 setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg-server
  $ cd repo-hg-server
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma
  $ hg bookmark master_bookmark -r 'tip'

verify content
  $ hg log
  changeset:   0:0e7ec5675652
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
blobimport the repo
  $ cd $TESTTMP
  $ blobimport repo-hg-server/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

setup the client repo
  $ cd $TESTTMP
  $ hgclone_treemanifest ssh://user@dummy/repo-hg-server client --noupdate --config extensions.remotenames= -q

create new hg commits
  $ cd $TESTTMP/client
  $ hg up -q 0
  $ echo b > b && hg ci -Am b
  adding b

try doing a non-pushrebase push with the new commits
  $ hgmn push --force ssh://user@dummy/repo
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "Pure pushes are disallowed in this repo",
  remote:     }
  remote:   Caused by:
  remote:     While resolving Changegroup
  remote:   Caused by:
  remote:     Pure pushes are disallowed in this repo
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

try doing a pushrebase push with the new commits
  $ hgmn push ssh://user@dummy/repo --config extensions.pushrebase= --config extensions.remotenames= --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
