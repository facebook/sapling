#chg-compatible
#require git gpg2 no-windows

  $ setconfig workingcopy.ruststatus=False
  $ . $TESTDIR/git.sh
  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ setconfig diff.git=true ui.allowemptycommit=true

Prepare a git repo:

  $ git init -q gitrepo
  $ cd gitrepo
  $ git config core.autocrlf false
  $ echo 1 > alpha
  $ git add alpha
  $ git commit -q -malpha

  $ echo 2 > beta
  $ git add beta
  $ git commit -q -mbeta

Clone a Sapling repo from a Git repo:

  $ cd $TESTTMP
  $ hg clone --git "file://$TESTTMP/gitrepo" repo1
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new ref]         3f5848713286c67b8a71a450e98c7fa66787bde2 -> remote/master
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo1

  $ hg log -Gr 'all()' -T '{node} {desc}'
  @  3f5848713286c67b8a71a450e98c7fa66787bde2 beta
  │
  o  b6c31add3e60ded7a9c9c803641edffb1dccd251 alpha
  
Create a GPG key and configure signing.

  $ export HGUSER="Test User <testuser@example.com>"
  $ gpg --batch --passphrase '' --quick-gen-key "$HGUSER" default default 2> /dev/null
  $ KEYID=$(gpg --list-secret-keys --keyid-format LONG --no-auto-check-trustdb | grep -oP '^sec\s+ rsa2048/\K(\w+)') 2> /dev/null
  gpg: please do a --check-trustdb
  $ hg config --local gpg.key "$KEYID"
  updated config in $TESTTMP/repo1/.hg/hgrc

Create a signed commit.

  $ echo 1 > gamma
  $ hg add gamma
  $ hg ci -m gamma
  $ git --git-dir .hg/store/git log --show-signature $(hg whereami) | grep -A 1 'gpg: Good'
  gpg: Good signature from "Test User <testuser@example.com>" [ultimate]
  Author: Test User <testuser@example.com>
