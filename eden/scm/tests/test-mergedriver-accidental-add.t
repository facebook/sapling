  $ enable mergedriver rebase

  $ newclientrepo
  $ cat > $TESTTMP/mergedriver.py << EOF
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels):
  >     for f in mergestate.unresolved():
  >         mergestate.mark(f, "d")
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels):
  >     for f in mergestate.driverresolved():
  >         mergestate.queueadd(f) # oops
  >         mergestate.mark(f, "r")
  > EOF
  $ setconfig experimental.mergedriver=python:$TESTTMP/mergedriver.py
  $ drawdag <<EOS
  > A # A/foo = A
  > B # B/foo = B
  > EOS
  $ hg rebase -q -r $A -d $B
  $ hg st
