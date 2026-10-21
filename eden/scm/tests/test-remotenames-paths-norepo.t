#require no-eden

`paths` is an `optionalrepo` command, so outside any repository dispatch passes
`repo=None`. remotenames is always on and wraps `paths` with `expaths`, which
dereferences `repo.localvfs` before looking at any option.

  $ cd $TESTTMP
  $ hg root
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [255]

Listing paths outside a repository should behave like the core command (no
paths configured: print nothing, exit 0; a NAME that is not configured: "not
found!", exit 1):

  $ hg paths
  $ hg paths default
  not found!
  [1]

Paths from the user config are listed outside a repository:

  $ setconfig paths.foo=http://x
  $ hg paths
  foo = http://x/
  $ hg paths foo
  http://x/
  $ hg paths -q
  foo

--add / --delete need a repository config file, so they abort cleanly:

  $ hg paths --add bar http://y
  abort: hg paths --add/--delete requires a repository
  [255]
  $ hg paths --delete foo
  abort: hg paths --add/--delete requires a repository
  [255]
  $ hg paths
  foo = http://x/

Inside a repository nothing changes:

  $ newclientrepo repo
  $ hg paths -a bar http://y
  $ hg paths
  bar = http://y/
  default = test:repo_server
  foo = http://x/
  $ hg paths -d bar
  $ hg paths -q
  default
  foo
