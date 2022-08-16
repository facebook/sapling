#chg-compatible
#require git no-windows
#debugruntest-compatible

  $ . $TESTDIR/git.sh

Server repo

  $ hg init --git server-repo
  $ cd server-repo
  $ drawdag << 'EOS'
  > C D
  > |/
  > B
  > |
  > A
  > EOS
  $ hg bookmark -r $B main

Client repo

  $ cd
  $ hg clone -q --git "$TESTTMP/server-repo/.hg/store/git" client-repo
  $ cd client-repo
  $ hg log -Gr: -T '{desc} {remotenames} {phase}'
  @  B remote/main public
  │
  o  A  public
  
Auto pull by node

  $ hg log -r $C -T '{desc}\n'
  pulling '06625e541e5375ee630d4bc10780e8d8fbfa38f9' from * (glob)
  C

Pull by node

  $ hg pull -qr $D

  $ hg log -Gr: -T '{desc} {remotenames} {phase}'
  o  D  draft
  │
  │ o  C  draft
  ├─╯
  @  B remote/main public
  │
  o  A  public
  
