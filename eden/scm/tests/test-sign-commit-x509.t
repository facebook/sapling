#require git openssl no-windows
#debugruntest-incompatible

  $ export HGIDENTITY=sl
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
  


Create a self-signed X.509 certificate for testing:

  $ export HGUSER="Test User <testuser@example.com>"
  $ openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 \
  >   -keyout "$TESTTMP/x509key.pem" -out "$TESTTMP/x509cert.pem" \
  >   -days 1 -nodes -subj "/CN=Test User/emailAddress=testuser@example.com" 2>/dev/null

Create a combined PEM file (cert + key):

  $ cat "$TESTTMP/x509cert.pem" "$TESTTMP/x509key.pem" > "$TESTTMP/x509combined.pem"

Configure X.509 signing with openssl (auto-detected):

  $ sl config --local signing.backend x509
  updated config in $TESTTMP/repo1/.sl/config
  $ sl config --local signing.key "$TESTTMP/x509combined.pem"
  updated config in $TESTTMP/repo1/.sl/config

Create a signed commit using combined PEM (cert + key in one file):

  $ echo 1 > gamma
  $ sl add gamma
  $ sl ci -m gamma

Verify the commit has an embedded signature (gpgsig header in git object):

  $ git --git-dir .sl/store/git cat-file commit $(sl whereami) | grep -c 'BEGIN SIGNED MESSAGE\|BEGIN CMS\|BEGIN PKCS7'
  1

Test with separate cert and key files:

  $ sl config --local signing.x509.certfile "$TESTTMP/x509cert.pem"
  updated config in $TESTTMP/repo1/.sl/config
  $ sl config --local signing.key "$TESTTMP/x509key.pem"
  updated config in $TESTTMP/repo1/.sl/config
  $ echo 2 > delta
  $ sl add delta
  $ sl ci -m delta

Verify the separate cert/key commit also has a signature:

  $ git --git-dir .sl/store/git cat-file commit $(sl whereami) | grep -c 'BEGIN SIGNED MESSAGE\|BEGIN CMS\|BEGIN PKCS7'
  1

Test error with a bad key path:

  $ sl config --local signing.key "/nonexistent/bad_key.pem"
  updated config in $TESTTMP/repo1/.sl/config
  $ echo 3 > epsilon
  $ sl add epsilon
  $ sl commit -m epsilon
  abort: signing key file not found: /nonexistent/bad_key.pem
  (ensure signing.key points to a valid PEM file containing your certificate and private key)
  [255]

Test with unsupported x509.format gives a clear error:

  $ sl config --local signing.backend x509
  updated config in $TESTTMP/repo1/.sl/config
  $ sl config --local signing.x509.format badformat
  updated config in $TESTTMP/repo1/.sl/config
  $ sl commit -m epsilon
  abort: unsupported signing.x509.format: badformat (expected 'openssl' or 'gpgsm')
  [255]

Test with unsupported backend gives a clear error:

  $ sl config --local signing.backend badbackend
  updated config in $TESTTMP/repo1/.sl/config
  $ sl commit -m epsilon
  abort: unsupported signing backend: badbackend (expected 'gpg', 'ssh', or 'x509')
  [255]
