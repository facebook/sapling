#debugruntest-compatible

  $ . "$TESTDIR/library.sh"

  $ eagerepo

  $ enable logginghelper
  $ enable sampling
  $ setconfig sampling.key.logginghelper=logginghelper

  $ hg init repo123
  $ cd repo123

  $ export SCM_SAMPLING_FILEPATH="$TESTTMP/sample"

  >>> def get_repo():
  ...     import json, sys, os
  ...     path = os.getenv("SCM_SAMPLING_FILEPATH")
  ...     with open(path, "rb") as f:
  ...         content = f.read()
  ...     os.unlink(path)
  ...     for line in content.split(b"\0"):
  ...         obj = json.loads(line.decode())
  ...         repo = obj.get("data", {}).get("repo")
  ...         if repo:
  ...             return repo


Check we got the repository name from the local path

  $ hg addremove

  >>> get_repo()
  'repo123'

Check that it doesn't matter where we are in the repo

  $ mkdir foobar
  $ cd foobar
  $ hg addremove
  $ hg status

  >>> get_repo()
  'repo123'

  $ cd ..

Check we got the repository name from the remote path

  $ setconfig paths.default=ssh://foo.com//bar/repo456

  $ hg addremove

  >>> get_repo()
  'repo456'

