#require no-eden

`paths` is an `optionalrepo` command, so outside any repository dispatch passes
`repo=None`. remotenames is always on and wraps `paths` with `expaths`, which
dereferences `repo.localvfs` before looking at any option.

  $ cd $TESTTMP
  $ hg root
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [255]

BUG: listing paths outside a repository crashes with an uncaught AttributeError
instead of behaving like the core command (print nothing / "not found!"):

  $ hg paths 2>&1 | grep -E '^AttributeError'
  AttributeError: 'NoneType' object has no attribute 'localvfs'
  $ hg paths default 2>&1 | grep -E '^AttributeError'
  AttributeError: 'NoneType' object has no attribute 'localvfs'

BUG: paths from the user config are not listed outside a repository either:

  $ setconfig paths.foo=http://x
  $ hg paths 2>&1 | grep -E '^AttributeError'
  AttributeError: 'NoneType' object has no attribute 'localvfs'
  $ hg paths foo 2>&1 | grep -E '^AttributeError'
  AttributeError: 'NoneType' object has no attribute 'localvfs'
  $ hg paths -q 2>&1 | grep -E '^AttributeError'
  AttributeError: 'NoneType' object has no attribute 'localvfs'

BUG: --add / --delete crash too, instead of a clean abort:

  $ hg paths --add bar http://y 2>&1 | grep -E '^AttributeError'
  AttributeError: 'NoneType' object has no attribute 'localvfs'
  $ hg paths --delete foo 2>&1 | grep -E '^AttributeError'
  AttributeError: 'NoneType' object has no attribute 'localvfs'
  $ hg paths 2>&1 | grep -E '^AttributeError'
  AttributeError: 'NoneType' object has no attribute 'localvfs'

Inside a repository everything works:

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
