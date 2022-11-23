#chg-compatible
#require git no-windows execbit
#debugruntest-compatible

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

  $ hg push -q -r 'desc(A)' --to main --create
  $ hg log -r remote/main -T '{desc}\n'
  A

Hook to reject push

  $ cat >> "$TESTTMP/server-repo.git/hooks/pre-receive" << EOF
  > echo "Push rejected by hook!" 1>&2
  > false
  > EOF
  $ chmod +x "$TESTTMP/server-repo.git/hooks/pre-receive"

Push B

  $ hg push -q -r 'desc(B)' --to main --create
  remote: Push rejected by hook!* (glob)
  To $TESTTMP/server-repo.git
   ! [remote rejected] 0de30934572f96ff6d3cbfc70aa8b46ef95dbb42 -> main (pre-receive hook declined)
  error: failed to push some refs to '$TESTTMP/server-repo.git'
  [1]

`remote/main` does not move to `B`

  $ hg log -r remote/main -T '{desc}\n'
  A
