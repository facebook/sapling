#chg-compatible
#require git ssh-keygen no-windows

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
  \xe2\x94\x82 (esc)
  o  b6c31add3e60ded7a9c9c803641edffb1dccd251 alpha
  


Create an SSH key and configure signing.

  $ export HGUSER="Test User <testuser@example.com>"
  $ ssh-keygen -t ed25519 -f "$TESTTMP/test_sign_key" -N "" -q
  $ sl config --local signing.backend ssh
  updated config in $TESTTMP/repo1/.sl/config
  $ sl config --local signing.key "$TESTTMP/test_sign_key"
  updated config in $TESTTMP/repo1/.sl/config

Create a signed commit.

  $ echo 1 > gamma
  $ sl add gamma
  $ sl ci -m gamma

Verify the SSH signature using git verify-commit:

  $ printf "%s %s\n" "testuser@example.com" "$(cat $TESTTMP/test_sign_key.pub)" > "$TESTTMP/allowed_signers"
  $ git --git-dir .sl/store/git -c gpg.format=ssh -c gpg.ssh.allowedSignersFile="$TESTTMP/allowed_signers" verify-commit $(sl whereami)
  Good "git" signature for testuser@example.com with * key * (glob)

Test SSH error with a bad key path.

  $ sl config --local signing.key "/nonexistent/bad_key"
  updated config in $TESTTMP/repo1/.sl/config
  $ echo 1 > delta
  $ sl commit -m delta
  abort: signing key file not found: /nonexistent/bad_key
  (ensure signing.key points to a valid SSH private key file)
  [255]
