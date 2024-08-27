#require git no-windows no-eden

  $ . $TESTDIR/git.sh

Prepare git repos

  $ git init -q -b main git-repo
  $ cd git-repo
  $ touch a
  $ git add a
  $ git commit -q -m commit1

  $ cd
  $ git clone -q git-repo git-repo2
  $ cd git-repo2

The remote url can be observed by sl:

  $ sl paths
  default = $TESTTMP/git-repo

Changes to remote is detected:

  $ git remote add upstream ssh://example.com/git-repo
  $ git remote set-url --push origin ssh://example.com/push-url
  $ sl paths
  default = $TESTTMP/git-repo
  default-push = ssh://example.com/push-url
  upstream = ssh://example.com/git-repo

ui.username can be synced from git:

  $ git config --local user.name Foo
  $ git config --local user.email foo@example.com
  $ sl config ui.username
  Foo <foo@example.com>

Scp-like URL is translated to ssh URL:

  $ git remote add myfork foo@bar:baz/repo1.git
  $ sl paths myfork
  ssh://foo@bar/baz/repo1.git
