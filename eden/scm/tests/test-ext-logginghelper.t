#require jq
#chg-compatible

  $ . "$TESTDIR/library.sh"

  $ enable logginghelper
  $ enable sampling
  $ setconfig sampling.key.logginghelper=logginghelper

  $ hg init repo123
  $ cd repo123

  $ export SCM_SAMPLING_FILEPATH="$TESTTMP/sample"

Check we got the repository name from the local path

  $ hg status
  $ tr '\0' '\n' < "$SCM_SAMPLING_FILEPATH" | jq -r .data.repo
  repo123
  $ rm "$SCM_SAMPLING_FILEPATH"

Check that it doesn't matter where we are in the repo

  $ mkdir foobar
  $ cd foobar
  $ hg status
  $ tr '\0' '\n' < "$SCM_SAMPLING_FILEPATH" | jq -r .data.repo
  repo123
  $ rm "$SCM_SAMPLING_FILEPATH"
  $ cd ..

Check we got the repository name from the remote path

  $ setconfig paths.default=ssh://foo.com//bar/repo456

  $ hg status
  $ tr '\0' '\n' < "$SCM_SAMPLING_FILEPATH" | jq -r .data.repo
  repo456
  $ rm "$SCM_SAMPLING_FILEPATH"
