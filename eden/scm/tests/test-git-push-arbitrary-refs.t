#require git no-eden

  $ . $TESTDIR/git.sh

Server repo

  $ git init -q --bare -b main server-repo.git

Client repo

  $ hg clone -q --git "$TESTTMP/server-repo.git" client-repo
  $ cd client-repo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

Push A

  $ hg push -q -r 'desc(A)' --to refs/test/test123 --create

Push B
  $ hg push -q -r 'desc(A)' --to refs/commitcloud/upload --create
  
Inspect repo
  $ cd "$TESTTMP/server-repo.git"
  $ git show-ref
  73c8ee0cae8ffb843cc154c3bf28a12438801d3f refs/commitcloud/upload
  73c8ee0cae8ffb843cc154c3bf28a12438801d3f refs/test/test123
