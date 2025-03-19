#require git no-eden gpg gpg2

  $ . $TESTDIR/git.sh

Prepare a gpg key for signing:

  $ export HGUSER='Test User <test@example.com>'
  $ export GNUPGHOME="$TESTTMP/gpghome"
  $ mkdir "$GNUPGHOME"
  $ chmod 0700 "$GNUPGHOME"
  $ gpg --quiet --batch --passphrase '' --quick-generate-key "$HGUSER" default default 0
  $ PUB_KEY_NAME=$(gpg --quiet --list-keys --with-colons | grep '^pub:' | sed 's/^([^:]*:){4}([^:]*).*/\2/')
  $ setconfig gpg.key=$PUB_KEY_NAME ui.allowemptycommit=true

Prepare a git repo:

  $ git init -q -b main git-repo
  $ cd git-repo

Commit via sl with gpg signing:

  $ sl commit -m init

Verify signature:

  $ git verify-commit $(sl log -r. -T '{node}')
  ...
  gpg: Good signature from "Test User <test@example.com>" [ultimate]

Edit commit message:

  $ sl metaedit -m init2

Verify signature after metaedit:

  $ git verify-commit $(sl log -r. -T '{node}')
  ...
  gpg: Good signature from "Test User <test@example.com>" [ultimate]

