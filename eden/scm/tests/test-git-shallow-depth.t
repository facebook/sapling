#require git no-windows

Test git shallow support.
"shallow" in terms of a git terminology.

  $ . $TESTDIR/git.sh

Create a repo with a few commits:

  $ git -c init.defaultBranch=main init -q repo-with-5-commits
  $ cd repo-with-5-commits
  $ drawdag << 'EOS'
  > A..E # bookmark main = E
  > EOS

Clone the repo but only keep the last 2 commits:

  $ cd
  $ git clone -q --depth 2 file://$PWD/repo-with-5-commits repo-shallow

The shallow repo can be read by sl:
(FIXME: it cannot be read yet)

  $ cd repo-shallow
  $ sl log -GT '{desc} {remotenames}'
  @  E origin/main
  │
  o  D
