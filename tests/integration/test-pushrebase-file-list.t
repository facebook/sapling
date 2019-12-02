  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export PUSHREBASE_REWRITE_DATES=1

  $ BLOB_TYPE="blob:files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'

Push a directory
  $ hg up -q 0
  $ mkdir dir
  $ echo 1 > dir/1
  $ echo 2 > dir/2
  $ echo 3 > dir/3
  $ hg -q addremove
  $ hg ci -m 'create dir'
  $ hgmn push -r . --to master_bookmark -q
  $ hgmn up master_bookmark -q

Now replace directory with a file and push it. Make sure file lists before push
and after push match
  $ hg rm dir
  removing dir/1
  removing dir/2
  removing dir/3
  $ echo dir > dir
  $ hg addremove -q
  $ hg ci -m 'replace directory with a file'

List of files before the push
  $ hg log -r . -T '{files}'
  dir dir/1 dir/2 dir/3 (no-eol)

  $ hgmn push -r . --to master_bookmark -q
  $ hgmn up master_bookmark -q

List of files after the push.
  $ hg log -r . -T '{files}'
  dir dir/1 dir/2 dir/3 (no-eol)
