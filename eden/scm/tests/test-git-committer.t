#require git no-windows
#debugruntest-compatible

Test that committer and author can be set separately. Committer is updated when
rewriting commits.

  $ configure modern
  $ enable rebase absorb
  $ . $TESTDIR/git.sh

  $ dump_commits() {
  >   local revs="${1}"
  >   sl log -r "${revs:-all()}"  -T '{desc}:\n date: {date|hgdate}\n author: {author}\n author date: {authordate|hgdate}\n committer: {committer}\n committer date: {committerdate|hgdate}\n'
  > }

Simple case: a single commit.

  $ newrepo '' --git
  $ drawdag << 'EOS'
  > A1
  > EOS
  $ sl go -q "desc('A1')"

Explicitly set author and committer:

  $ sl metaedit -d '3600 -120' -m A2 -u 'user2' --config git.committer='user3' --config git.committer-date='7200 60'
  $ dump_commits
  A2:
   date: 7200 60
   author: user2 <>
   author date: 3600 -120
   committer: user3 <>
   committer date: 7200 60

Test templates:

  $ sl log -T '{author}\n{authordate|isodatesec}\n{committer}\n{committerdate|isodatesec}\n'
  user2 <>
  1970-01-01 01:02:00 +0002
  user3 <>
  1970-01-01 01:59:00 -0001

Metaedit without `-d` does not update author date
"date" is the max of author and committer dates:

  $ sl metaedit -u user22
  $ dump_commits
  A2:
   date: 3600 -120
   author: user22 <>
   author date: 3600 -120
   committer: test <>
   committer date: 0 0
  >>> assert 'author date: 3600 -120' in _

Metaedit with `-d` does not update committer date to the specified value:

  $ sl metaedit -d '4800 -240'
  $ dump_commits
  A2:
   date: 4800 -240
   author: user22 <>
   author date: 4800 -240
   committer: test <>
   committer date: 0 0
  >>> assert 'committer date: 4800' not in _

If committer and date are not explicitly set, the current author ('test', set
by the test runner via HGUSER) and the current wall time are used:

  $ sl metaedit -m A3 --config git.committer= --config git.committer-date=
  $ dump_commits
  A3:
   date: * (glob)
   author: user22 <>
   author date: 0 0
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
   date: 0 0
   author: test <>
   author date: 0 0
   committer: test <>
   committer date: 0 0
  B:
   date: 0 0
   author: test <>
   author date: 0 0
   committer: test <>
   committer date: 0 0
  C:
   date: 0 0
   author: test <>
   author date: 0 0
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
   date: 0 0
   author: test <>
   author date: 0 0
   committer: test <>
   committer date: 0 0
  B:
   date: 1670818320 -28800
   author: user <user@example.com>
   author date: 0 0
   committer: committer <committer@example.com>
   committer date: 1670818320 -28800
  C:
   date: 1670818320 -28800
   author: test <>
   author date: 0 0
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
   date: 0 0
   author: test <>
   author date: 0 0
   committer: test <>
   committer date: 0 0
  $ sl rebase -qr $C -d $A --config git.committer=user5
  $ dump_commits "successors($C)"
  C:
   date: 0 0
   author: test <>
   author date: 0 0
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
   date: 0 0
   author: test <>
   author date: 0 0
   committer: test <>
   committer date: 0 0
  B:
   date: 0 0
   author: test <>
   author date: 0 0
   committer: user6 <>
   committer date: 0 0
  C:
   date: 0 0
   author: test <>
   author date: 0 0
   committer: user6 <>
   committer date: 0 0
