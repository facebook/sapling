#chg-compatible

Test adding, removing, changing files in both merge parents, without telling
mergedriver the exact file list to change at "preprocess" time.

  $ enable mergedriver

  $ newrepo
  $ drawdag << 'EOS'
  > B C  # C/A=1
  > |/   # B/A=2
  > A    # C/C_del=C
  > |    # B/B_del=B
  > Z    # C/C_change=C
  >      # B/B_change=B
  > EOS
  $ hg up -q $B

The merge driver wants to delete B_del and C_del, change B_change and C_change,
and add B_add and C_add. Note: there are no conflicts.

  $ setconfig experimental.mergedriver=python:$TESTTMP/mergedriver-test.py

  $ cat > $TESTTMP/mergedriver-test.py << EOF
  > from edenscm.mercurial import node
  > from mercurial import node as node2
  > assert node is node2
  > import os
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels):
  >     from edenscm.mercurial import util
  >     from mercurial import util as util2
  >     assert util is util2
  >     ui.write("merge driver preprocess\n")
  >     # Right now, need to mark at least one file to get mergedriver running
  >     mergestate.mark("A", "d")  # driver-resovled
  >     # Intentionally not marking all touched files as "driver-resolved", to
  >     # emulate some practical use-cases where it is impossible to know the
  >     # file list before hand.
  > 
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels):
  >     ui.write("merge driver conclude\n")
  > 
  >     # emulating an external script making changes to the working copy
  >     os.unlink("A")
  >     os.unlink("B_del")
  >     os.unlink("C_del")
  > 
  >     open("B_add", "w").write("B")
  >     open("C_add", "w").write("C")
  > 
  >     open("B_change", "a").write("B")
  >     open("C_change", "a").write("C")
  > 
  >     # mark files using mergedriver APIs
  >     mergestate.queueremove("A")
  >     mergestate.queueremove("B_del")
  >     mergestate.queueremove("C_del")
  >     mergestate.queueadd("B_add")
  >     mergestate.queueadd("C_add")
  >     mergestate.queueget("B_change")
  >     mergestate.queueget("C_change")
  > EOF

Do the merge:

  $ hg graft $C
  grafting cb95dc195621 "C"
  merge driver preprocess
  merge driver conclude

Status should be clean:

  $ hg status

Working copy and commit made should have expected changes:

  >>> import glob
  >>> for path in sorted(glob.glob("*")):
  ...     print("%s: %s" % (path, open(path).read().strip()))
  B: B
  B_add: B
  B_change: BB
  C: C
  C_add: C
  C_change: CC
  Z: Z

  $ hg diff -r 'p1(.)' -r '.' --stat
   A        |  1 -
   B_add    |  1 +
   B_change |  2 +-
   B_del    |  1 -
   C        |  1 +
   C_add    |  1 +
   C_change |  1 +
   7 files changed, 5 insertions(+), 3 deletions(-)
