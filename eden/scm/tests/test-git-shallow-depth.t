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
  abort: When constructing alloc::boxed::Box<dyn commits_trait::DagCommits + core::marker::Send> from dyn storemodel::StoreInfo, "10-git-commits" reported error
  
  Caused by:
      0: reading git commit
      1: object not found - no match for id (06625e541e5375ee630d4bc10780e8d8fbfa38f9); class=Odb (9); code=NotFound (-3)
  ...
