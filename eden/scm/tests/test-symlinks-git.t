#debugruntest-compatible
#require git symlink

  $ configure modernclient
  $ . $TESTDIR/git.sh

Test cloning git repos
  $ git init symlinksgit -q
  $ cd symlinksgit
  $ git config core.symlinks true
  $ git config core.autocrlf false
  $ mkdir foo
  $ echo saluton > foo/bar
  $ ln -s foo/bar salutonlink
  $ git add -A && git commit -am "git commit with symlinks" -q
  $ cd ..
  $ hg clone --git "$TESTTMP/symlinksgit" clientrepo3 -q
  $ readlink clientrepo3/salutonlink
  foo/bar
  $ cat clientrepo3/salutonlink
  saluton
