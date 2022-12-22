#require git no-windows
#debugruntest-compatible
#chg-compatible

Test that committer and author can be set separately. Committer is updated when
rewriting commits.

  $ configure modern
  $ enable rebase absorb
  $ . $TESTDIR/git.sh

  $ dump_commits() {
  >   local revs="${1}"
  >   sl log -r "${revs:-all()}"  -T '{desc}:\n author: {author}\n committer: {get(extras,"committer")}\n committer date: {get(extras,"committer_date")}\n'
  > }

Simple case: a single commit.

  $ newrepo '' --git
  $ drawdag << 'EOS'
  > A1
  > EOS

Explicitly set author and committer:

  $ sl metaedit -r 'desc(A1)' -m A2 -u 'user2' --config git.committer='user3' --config git.committer-date='10 10'
  $ dump_commits
  A2:
   author: user2 <>
   committer: user3 <>
   committer date: 10 0

If committer and date are not explicitly set, the current author ('test', set
by the test runner via HGUSER) and the current wall time are used:

  $ sl metaedit -r 'desc(A2)' -m A3 --config git.committer= --config git.committer-date=
  $ dump_commits
  A3:
   author: user2 <>
   committer: test <>
   committer date: [1-9][0-9]+ 0 (re)

A more complex case - a stack of 3 commits.

  $ newrepo '' --git
  $ drawdag << EOS
  > C
  > :
  > A
  > EOS

  $ dump_commits
  A:
   author: test <>
   committer: test <>
   committer date: 0 0
  B:
   author: test <>
   committer: test <>
   committer date: 0 0
  C:
   author: test <>
   committer: test <>
   committer date: 0 0

Edit B (middle of the stack), and trigger an auto rebase (a rewrite of C):

  $ sl metaedit -r $B -u 'user <user@example.com>' --config git.committer='committer <committer@example.com>' --config git.committer-date='2022-12-12 12:12 +0800'

Result:
- Committer and committer date are used for newly created commits (new B and new C)
- Author is changed per request for B.
- Other commits (A) remain unchanged.

  $ dump_commits
  A:
   author: test <>
   committer: test <>
   committer date: 0 0
  B:
   author: user <user@example.com>
   committer: committer <committer@example.com>
   committer date: 1670818320 -28800
  C:
   author: test <>
   committer: committer <committer@example.com>
   committer date: 1670818320 -28800

Rebase uses the current committer, not from the existing commits.

  $ newrepo '' --git
  $ drawdag << 'EOS'
  > C
  > :
  > A
  > EOS
  $ dump_commits $C
  C:
   author: test <>
   committer: test <>
   committer date: 0 0
  $ sl rebase -qr $C -d $A --config git.committer=user5
  $ dump_commits "successors($C)"
  C:
   author: test <>
   committer: user5 <>
   committer date: 0 0

Absorb updates committer too. In this test we edit commit B and C, leaving A unchanged.

  $ newrepo '' --git
  $ drawdag << 'EOS'
  > C
  > :
  > A
  > EOS

  $ sl go -q $C
  $ echo 1 >> B

  $ sl absorb -qa --config git.committer=user6

(B and C should have the new committer)
  $ dump_commits
  A:
   author: test <>
   committer: test <>
   committer date: 0 0
  B:
   author: test <>
   committer: user6 <>
   committer date: 0 0
  C:
   author: test <>
   committer: user6 <>
   committer date: 0 0
