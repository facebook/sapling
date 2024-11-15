  $ enable rebase mergedriver undo

  $ setconfig drawdag.defaultfiles=false

  $ cat > mergedriver.py << EOF
  > print("EXPENSIVE")
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels):
  >    print("PREPROCESS")
  >    pass
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels):
  >    print("CONCLUDE")
  >    pass
  > EOF
  $ setconfig experimental.mergedriver="python:$TESTTMP/mergedriver.py"

In-memory rebase + merge driver not used:
  $ newclientrepo
  $ drawdag <<EOS
  >    D  # D/bar = 1\n2\n4\n
  >    |  # C/foo = a\nb\nC\n
  > B  C  # B/bar = 2\n2\n3\n
  > | /   # B/foo = A\nb\nc\n
  > |/    # A/bar = 1\n2\n3\n
  > A     # A/foo = a\nb\nc\n
  > EOS
Don't reload the merge driver for every commit:
  $ hg rebase -q -s $C -d $B --config rebase.experimental.inmemory=true
  EXPENSIVE
  PREPROCESS
  PREPROCESS

On-disk rebase + merge driver not used:
  $ hg undo -q
  $ hg rebase -q -s $C -d $B --config rebase.experimental.inmemory=false
  EXPENSIVE
  PREPROCESS
  EXPENSIVE
  PREPROCESS

