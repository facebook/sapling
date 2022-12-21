#chg-compatible
#require git no-windows
#debugruntest-compatible

Test the 'revert' command with submodules:

  $ configure modern
  $ setconfig diff.git=1
  $ . $TESTDIR/git.sh
  $ enable rebase

Prepare submodule and main repo:

  $ sl init --git sub
  $ drawdag --cwd sub << 'EOS'
  > S5
  > :
  > S1
  > EOS

  $ sl init --git main
  $ drawdag --cwd main --no-files << EOS
  > B      # B/m=$S2 (submodule)
  > |      # A/m=$S1 (submodule)
  > A      # A/.gitmodules=[submodule "m"]\n path=m\n url=file://$TESTTMP/sub/.sl/store/git
  > EOS

Revert submodule changes in working copy:

  $ cd ~/main
  $ sl go -q $B

- no-op revert

  $ sl revert m
  $ sl st

- revert to current commit

  $ sl --cwd ~/sub -q checkout $S3
  $ sl st
  $ sl d

  $ sl revert m

  $ sl st
  $ sl d

- revert to different commit

  $ sl up -qC .
  $ sl revert -r '.^' m

  $ sl st
  M m
  $ sl d
  diff --git a/m b/m
  --- a/m
  +++ b/m
  @@ -1,1 +1,1 @@
  -Subproject commit b1eae93731683dc9cf99f3714f5b4a23c6b0b13b
  +Subproject commit 5d045cb6dd867debc8828c96e248804f892cf171

- reset to different commit

  $ sl --config extensions.reset= reset -kr '.^'
  $ sl st

Revert committed submodule changes:

  $ sl go -qC $B

- revert submodule change so commit B looks like commit A

  $ sl revert -r $A m
  $ sl amend
  $ sl cat -r . m
  Subproject commit 5d045cb6dd867debc8828c96e248804f892cf171
  $ sl cat -r $A m
  Subproject commit 5d045cb6dd867debc8828c96e248804f892cf171
  $ sl st

