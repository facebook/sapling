#debugruntest-compatible
  $ configure mutation-norecord
Test a mergedriver that raises exceptions in its preprocess() hook:

  $ newrepo
  $ enable amend rebase
  $ setconfig rebase.singletransaction=True
  $ setconfig rebase.experimental.inmemorywarning="rebasing in-memory!"

  $ mkdir driver
  $ cat > driver/__init__.py <<EOF
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     ui.write("in preprocess()\n")
  >     ui.flush()
  >     raise Exception("some exception in preprocess()")
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     pass
  > EOF

  $ setconfig experimental.mergedriver=python:driver/

Force a conflicted merge so driver will run:

  $ echo "base" > FILE
  $ hg add FILE
  $ hg commit -Aq -m "base commit"
  $ hg book -r . "base"

Now make two conflicting commits:

  $ echo "v1" > FILE
  $ hg com -m "A"
  $ hg book -r . A
  $ hg up -q .~1

  $ echo "conflict" > FILE
  $ hg com -m "B"
  $ hg book -r . B

Without IMM:
  $ hg rebase -r B -d A --config rebase.experimental.inmemory=0
  rebasing * "B" (B) (glob)
  in preprocess()
  error: preprocess hook raised an exception: some exception in preprocess()
  Traceback (most recent call last):
    # collapsed by devel.collapse-traceback
  Exception: some exception in preprocess()
  warning: merge driver failed to preprocess files
  (hg resolve --all to retry, or hg resolve --all --skip to skip merge driver)
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted

With IMM:
  $ hg rebase -r B -d A --config rebase.experimental.inmemory=1
  rebasing in-memory!
  rebasing * "B" (B) (glob)
  in preprocess()
  error: preprocess hook raised an exception: some exception in preprocess()
  Traceback (most recent call last):
    # collapsed by devel.collapse-traceback
  Exception: some exception in preprocess()
  warning: merge driver failed to preprocess files
  (hg resolve --all to retry, or hg resolve --all --skip to skip merge driver)
  hit merge conflicts (in FILE); switching to on-disk merge
  rebasing * "B" (B) (glob)
  in preprocess()
  error: preprocess hook raised an exception: some exception in preprocess()
  Traceback (most recent call last):
    # collapsed by devel.collapse-traceback
  Exception: some exception in preprocess()
  warning: merge driver failed to preprocess files
  (hg resolve --all to retry, or hg resolve --all --skip to skip merge driver)
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted

===============================================================================
Try a subtler case: a preprocess() exception that only happens in IMM mode.

This mimics a real-world merge driver; the exception comes from preprocess() trying
to resolve files directly, which can raise a InMemoryMergeConflictsError if there
are conflicts, but will work when in on-disk mode.

The tldr of this case is we can't just quit early if prepocess() is broken; we
have to try it both ways. (It might be nice to change that.)
===============================================================================

  $ hg up base
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark base)
  $ cat > driver/__init__.py <<EOF
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     ui.write("in preprocess()\n")
  >     for f in mergestate:
  >         mergestate.resolve(f, wctx)
  >     ui.write("done with preprocess()\n")
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     pass
  > EOF
  $ hg commit -m "new base"
  $ hg rebase -r A+B -d .
  rebasing * "A" (A) (glob)
  rebasing * "B" (B) (glob)

Without IMM, you can see we try to merge FILE twice (once in preprocess() and once later),
and it fails:
  $ hg rebase -r B -d A --config rebase.experimental.inmemory=0
  rebasing * "B" (B) (glob)
  in preprocess()
  warning: 1 conflicts while merging FILE! (edit, then use 'hg resolve --mark')
  done with preprocess()
  merging FILE
  warning: 1 conflicts while merging FILE! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat FILE
  <<<<<<< dest:   * A - test: A (glob)
  v1
  =======
  conflict
  >>>>>>> source: * B - test: B (glob)
  $ hg rebase --abort
  rebase aborted

With IMM, it's *very* noisy, but we do eventually get to the same place:
  $ hg rebase -r B -d A --config rebase.experimental.inmemory=1
  rebasing in-memory!
  rebasing * "B" (B) (glob)
  in preprocess()
  error: preprocess hook raised an exception: in-memory merge does not support merge conflicts
  Traceback (most recent call last):
    # collapsed by devel.collapse-traceback
  edenscm.mercurial.error.InMemoryMergeConflictsError: in-memory merge does not support merge conflicts
  warning: merge driver failed to preprocess files
  (hg resolve --all to retry, or hg resolve --all --skip to skip merge driver)
  hit merge conflicts (in FILE); switching to on-disk merge
  rebasing * "B" (B) (glob)
  in preprocess()
  warning: 1 conflicts while merging FILE! (edit, then use 'hg resolve --mark')
  done with preprocess()
  merging FILE
  warning: 1 conflicts while merging FILE! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat FILE
  <<<<<<< dest:   * A - test: A (glob)
  v1
  =======
  conflict
  >>>>>>> source: * B - test: B (glob)
