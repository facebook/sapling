#require git no-windows
#debugruntest-compatible
#chg-compatible

Dedicated test about rebase with submodule involved.

  $ configure modern
  $ . $TESTDIR/git.sh
  $ enable rebase

Test case 1: rebase destination has submodule changes.

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

  $ sl diff -r 'max(desc(E1))^' -r 'max(desc(E3))^' --stat
   E1 |  1 +
   E2 |  1 +
   2 files changed, 2 insertions(+), 0 deletions(-)

The rebased stack include the submodule change by commit B:

  $ sl cat -r $A m
  Subproject commit 5d045cb6dd867debc8828c96e248804f892cf171
  $ sl cat -r $B m
  Subproject commit b1eae93731683dc9cf99f3714f5b4a23c6b0b13b

  $ sl cat -r $E1 m
  Subproject commit 5d045cb6dd867debc8828c96e248804f892cf171
  $ sl cat -r 'max(desc(E1))' m
  Subproject commit b1eae93731683dc9cf99f3714f5b4a23c6b0b13b

Test case 2: the stack being rebased has submodule changes.

  $ cd
  $ sl init --git main2
  $ cd main2
  $ drawdag << EOS
  >   E3
  >   :    # E3/m=$S3 (submodule)
  > C E1   # E1/m=$S2 (submodule)
  > :/     # A/m=$S1 (submodule)
  > A      # A/.gitmodules=[submodule "m"]\n path=m\n url=file://$TESTTMP/sub/.sl/store/git
  > EOS

  $ sl goto $C
  pulling submodule m
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl rebase -qs $E1 -d $C

  $ for i in E1 E2 E3; do
  >   printf "$i: "
  >   sl --cwd ~/sub log -r $(sl cat -r "max(desc($i))" m | sed 's/.* //') -T '{desc}\n'
  > done
  E1: S2
  E2: S2
  E3: S3

Test case 3: the stack being rebased has conflicted submodule changes.

  $ cd
  $ sl init --git main3
  $ cd main3
  $ drawdag << EOS
  >   E3   # B/m=$S4 (submodule)
  >   :    # E3/m=$S3 (submodule)
  > C E1   # E1/m=$S2 (submodule)
  > :/     # A/m=$S1 (submodule)
  > A      # A/.gitmodules=[submodule "m"]\n path=m\n url=file://$TESTTMP/sub/.sl/store/git
  > EOS

  $ sl goto -q $C
  $ sl rebase -s $E1 -d $C
  rebasing * "E1" (glob)
  submodule 'm' changed by 'E1' is dropped due to conflict
  rebasing * "E2" (glob)
  rebasing * "E3" (glob)
  submodule 'm' changed by 'E3' is dropped due to conflict

  $ for i in E1 E2 E3; do
  >   printf "$i: "
  >   sl --cwd ~/sub log -r $(sl cat -r "max(desc($i))" m | sed 's/.* //') -T '{desc}\n'
  > done
  E1: S4
  E2: S4
  E3: S4
