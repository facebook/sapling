#require git no-eden

  $ unset RUST_BACKTRACE
  $ eagerepo
  $ . $TESTDIR/git.sh

Make sure that Windows is unable to check out paths with `..\` in their path.
  $ mkdir brokengitrepo
  $ tar -xf $TESTDIR/brokengitrepo.tar.gz -C $TESTTMP/brokengitrepo
#if windows
  $ hg clone --git "$TESTTMP/brokengitrepo" brokencopy
  From $TESTTMP/brokengitrepo
   * [new ref]         9ff0e959c6d6dec6f16d7ba9fcaa9ed407bf77d6 -> remote/master
  abort: error writing files:
   ..\windowstroublemaker.txt: invalid component in "..\windowstroublemaker.txt": invalid path component ".."
  [255]

#else
  $ hg clone --git "$TESTTMP/brokengitrepo" brokencopy
  From $TESTTMP/brokengitrepo
   * [new ref]         9ff0e959c6d6dec6f16d7ba9fcaa9ed407bf77d6 -> remote/master
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls -a brokencopy
  ..\windowstroublemaker.txt
  .hg
#endif
