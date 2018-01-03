  $ python -c 'from mercurial.dirstate import rootcache' || exit 80

Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ hg init

We should only read .gitignore files in a hg-git repo (i.e. one with .hg/git
directory) otherwise, a rogue .gitignore could slow down a hg-only repo

  $ mkdir .hg/git

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

  $ echo '.hg/' > .gitignore
  $ hg status
  ? .gitignore
  ? bar
  ? dir/.gitignore
  ? dir/bar
  ? foo
  ? foobar

  $ echo 'dir/.hg/' > .gitignore
  $ hg status
  ? .gitignore
  ? bar
  ? dir/.gitignore
  ? dir/bar
  ? foo
  ? foobar

  $ echo '.hg/foo' > .gitignore
  $ hg status
  ? .gitignore
  ? bar
  ? dir/.gitignore
  ? dir/bar
  ? foo
  ? foobar

  $ touch foo.hg
  $ echo 'foo.hg' > .gitignore
  $ hg status
  ? .gitignore
  ? bar
  ? dir/.gitignore
  ? dir/bar
  ? foo
  ? foobar
  $ rm foo.hg

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

  $ hg add .gitignore
  $ hg commit -m "add and commit .gitignore"
  $ rm .gitignore
  $ rm .hgignore
  $ hg status
  ! .gitignore
  ? bar
  ? dir/.gitignore
  ? dir/bar
  ? foo
  ? foobar

show pattern error in hgignore file as expected (issue197)
----------------------------------------------------------

  $ cat > $TESTTMP/invalidhgignore <<EOF
  > # invalid syntax in regexp
  > foo(
  > EOF
  $ hg status --config ui.ignore=$TESTTMP/invalidhgignore
  abort: $TESTTMP/invalidhgignore: invalid pattern (relre): foo(
  [255]

  $ cat > .hgignore <<EOF
  > # invalid syntax in regexp
  > foo(
  > EOF
  $ hg status
  abort: $TESTTMP/.hgignore: invalid pattern (relre): foo(
  [255]
