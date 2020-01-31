TODO: configure mutation
  $ configure noevolution
Test mergedriver invalidation with IMM.

  $ newrepo
  $ enable rebase
  $ setconfig rebase.singletransaction=True
  $ setconfig rebase.experimental.inmemory=True
  $ setconfig rebase.experimental.inmemorywarning="rebasing in-memory!"

  $ mkdir driver
  $ cat > driver/__init__.py <<EOF
  > from .generators import someFunction, VERSION
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     unresolved_files = list(mergestate.unresolved())
  >     ui.warn("generators version = %s\n" % VERSION)
  >     someFunction(repo)
  >     for unresolved_file in unresolved_files:
  >         mergestate.mark(unresolved_file, 'd')
  >     mergestate.commit()
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     pass
  > EOF

  $ cat > driver/generators.py <<EOF
  > VERSION = "BASE"
  > def someFunction(repo):
  >     repo.ui.warn("base's someFunction() called\n")
  >     pass
  > EOF
  $ setconfig experimental.mergedriver=python:driver/


A dummy file (FILE) is created to force a simple three-way merge (without
conflicts, though a conflict would work too). Otherwise, mergedriver won't run.

  $ $TESTDIR/seq.py 1 10 > FILE
  $ hg add FILE
  $ hg commit -Aq -m "base mergedriver"
  $ hg book -r . "base"

Next, off of BASE, make an API change to the driver.

  $ cat > driver/__init__.py <<EOF
  > from .generators import someFunction, VERSION
  > 
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     unresolved_files = list(mergestate.unresolved())
  >     ui.warn("generators version = %s\n" % VERSION)
  >     someFunction(repo, "new_required")
  > 
  >     for unresolved_file in unresolved_files:
  >         mergestate.mark(unresolved_file, 'd')
  > 
  >     mergestate.commit()
  > 
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     pass
  > EOF
  $ cat > driver/generators.py <<EOF
  > VERSION = "NEW"
  > 
  > def someFunction(repo, new_required_arg):
  >     print("new_required_arg = %s" % new_required_arg)
  >     pass
  > EOF
  $ $TESTDIR/seq.py 1 11 > FILE
  $ hg com -m "new driver"
  $ hg book -r . new_driver
  $ hg up -q .~1


Next make a change to the dummy file off BASE.
  $ hg up base
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark base)
  $ hg up -C .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark base)
  $ $TESTDIR/seq.py 0 10 > FILE
  $ hg commit -m "prefix FILE with 0"
  $ hg book -r . "base_1"

Rebase on top of the new driver, with the old driver checked out.
- We expect to see "generators version = BASE" as we run preprocess() with the old driver.
- Then, after restarting and using on-disk merge (and thus, checking out dest, which has the new driver),
we expect to see "generators version = NEW".
- If mergedriver isn't invalidated correctly, it'll say "generators version = BASE".

  $ hg rebase -d new_driver
  rebasing in-memory!
  rebasing 83615e50cada "prefix FILE with 0" (base_1)
  generators version = BASE
  base's someFunction() called
  artifact rebuild required (in FILE); switching to on-disk merge
  rebasing 83615e50cada "prefix FILE with 0" (base_1)
  generators version = NEW
  new_required_arg = new_required
  note: rebase of 2:* created no changes to commit (glob)
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/*-rebase.hg (glob)

