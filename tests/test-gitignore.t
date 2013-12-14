  $ python -c 'from mercurial.dirstate import rootcache' || exit 80
  $ python -c 'from mercurial.ignore import readpats' || exit 80

Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ hg init

  $ touch foo
  $ touch foobar
  $ touch bar
  $ echo 'foo*' > .gitignore
  $ hg status
  ? .gitignore
  ? bar

  $ echo '*bar' > .gitignore
  $ hg status
  ? .gitignore
  ? foo

  $ mkdir dir
  $ touch dir/foo
  $ echo 'foo' > .gitignore
  $ hg status
  ? .gitignore
  ? bar
  ? foobar

  $ echo '/foo' > .gitignore
  $ hg status
  ? .gitignore
  ? bar
  ? dir/foo
  ? foobar

  $ rm .gitignore
  $ echo 'foo' > dir/.gitignore
  $ hg status
  ? bar
  ? dir/.gitignore
  ? foo
  ? foobar

  $ touch dir/bar
  $ echo 'bar' > .gitignore
  $ hg status
  ? .gitignore
  ? dir/.gitignore
  ? foo
  ? foobar

  $ echo '/bar' > .gitignore
  $ hg status
  ? .gitignore
  ? dir/.gitignore
  ? dir/bar
  ? foo
  ? foobar

  $ echo 'foo*' > .gitignore
  $ echo '!*bar' >> .gitignore
  $ hg status
  .gitignore: unsupported ignore pattern '!*bar'
  ? .gitignore
  ? bar
  ? dir/.gitignore
  ? dir/bar

  $ touch .hgignore
  $ hg status
  ? .gitignore
  ? .hgignore
  ? bar
  ? dir/.gitignore
  ? dir/bar
  ? dir/foo
  ? foo
  ? foobar

  $ echo 'syntax: re' > .hgignore
  $ echo 'foo.*$(?<!bar)' >> .hgignore
  $ echo 'dir/foo' >> .hgignore
  $ hg status
  ? .gitignore
  ? .hgignore
  ? bar
  ? dir/.gitignore
  ? dir/bar
  ? foobar
