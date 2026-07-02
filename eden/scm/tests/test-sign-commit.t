#require git gpg no-eden no-windows

  $ . $TESTDIR/git.sh
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
  $ sl clone --git "file://$TESTTMP/gitrepo" repo1
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new ref]         3f5848713286c67b8a71a450e98c7fa66787bde2 -> remote/master
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo1

  $ sl log -Gr 'all()' -T '{node} {desc}'
  @  3f5848713286c67b8a71a450e98c7fa66787bde2 beta
  │
  o  b6c31add3e60ded7a9c9c803641edffb1dccd251 alpha
  
Create a GPG key and configure signing.

  $ export HGUSER="Test User <testuser@example.com>"
  $ gpg --batch --passphrase '' --yes --quick-gen-key "$HGUSER" rsa2048 default 2>/dev/null
  $ KEYID=$(gpg --list-secret-keys --keyid-format LONG --no-auto-check-trustdb | grep '^sec' | grep -o 'rsa2048/[0-9A-Fa-f][0-9A-Fa-f]*' | sed 's|rsa2048/||')
  gpg: please do a --check-trustdb
  $ sl config --local gpg.key "$KEYID"
  updated config in $TESTTMP/repo1/.sl/config

Create a signed commit.

  $ echo 1 > gamma
  $ sl add gamma
  $ sl ci -m gamma
  $ git --git-dir .sl/store/git log --show-signature $(sl whereami) | grep -A 1 'gpg: Good'
  gpg: Good signature from "Test User <testuser@example.com>" [ultimate]
  Author: Test User <testuser@example.com>

Test GPG error.

  $ sl config --local gpg.key "foobar"
  updated config in $TESTTMP/repo1/.sl/config
  $ echo 1 > delta
  $ sl commit -m gamma
  abort: error when running gpg with gpgkeyid foobar:
    gpg: skipped "foobar": * (glob)
    [GNUPG:] * (glob)
    [GNUPG:] * (glob)
    gpg: signing failed: * (glob)
  
  [255]
