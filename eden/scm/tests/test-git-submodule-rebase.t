#require git no-windows
#debugruntest-compatible
#chg-compatible

Dedicated test about rebase with submodule involved.

  $ configure modern
  $ . $TESTDIR/git.sh
  $ enable rebase

Prepare submodule and main repo:

  $ sl init --git sub
  $ drawdag --cwd sub << 'EOS'
  > S5
  > :
  > S1
  > EOS

In the main repo, E1..E3 is a feature stack, A..C is the main stack.
B makes a submodule change:

  $ sl init --git main
  $ drawdag --cwd main << EOS
  >   E3
  >   :
  > C E1   # B/m=$S2 (submodule)
  > :/     # A/m=$S1 (submodule)
  > A      # A/.gitmodules=[submodule "m"]\n path=m\n url=file://$TESTTMP/sub/.sl/store/git
  > EOS

Sanity check on submodule:

  $ cd ~/main
  $ sl goto -q $B
  $ cat m/S2
  S2 (no-eol)

Rebase the E1..E3 stack from A to C, so it is past the submodule change in B:

  $ sl rebase -qs $E1 -d $C

The rebased E stack itself should not include submodule changes:
BUG: m is included in rebase result!

  $ sl diff -r 'max(desc(E1))^' -r 'max(desc(E3))^' --stat
   E1 |  1 +
   E2 |  1 +
   m  |  2 +-
   3 files changed, 3 insertions(+), 1 deletions(-)

The rebased stack include the submodule change by commit B:
BUG: rebased stack still uses A not B submodule!

  $ sl cat -r $A m
  Subproject commit 5d045cb6dd867debc8828c96e248804f892cf171
  $ sl cat -r $B m
  Subproject commit b1eae93731683dc9cf99f3714f5b4a23c6b0b13b

  $ sl cat -r $E1 m
  Subproject commit 5d045cb6dd867debc8828c96e248804f892cf171
  $ sl cat -r 'max(desc(E1))' m
  Subproject commit 5d045cb6dd867debc8828c96e248804f892cf171

