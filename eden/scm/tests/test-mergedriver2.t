
#require no-eden


Test adding, removing, changing files in both merge parents, without telling
mergedriver the exact file list to change at "preprocess" time.

  $ enable mergedriver

  $ eagerepo
  $ sl init repo
  $ cd repo
  $ drawdag << 'EOS'
  > B C  # C/A=1
  > |/   # B/A=2
  > A    # C/C_del=C
  > |    # B/B_del=B
  > Z    # C/C_change=C
  >      # B/B_change=B
  > EOS
  $ sl up -q $B

The merge driver wants to delete B_del and C_del, change B_change and C_change,
and add B_add and C_add. Note: there are no conflicts.

  $ setconfig experimental.mergedriver=python:$TESTTMP/mergedriver-test.py

  $ cat > $TESTTMP/mergedriver-test.py << EOF
  > from sapling import node
  > import os
  > def print_p1(ui, repo):
  >     ui.write("  dirstate p1: %s\n" % (node.hex(repo.localvfs.tryread("dirstate")[:len(node.nullid)]),))
  >     ui.write("  repo['.']  : %s\n" % (repo['.'].hex(),))
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels):
  >     from sapling import util
  >     ui.write("merge driver preprocess\n")
  >     print_p1(ui, repo)
  >     # Right now, need to mark at least one file to get mergedriver running
  >     mergestate.mark("A", "d")  # driver-resovled
  >     # Intentionally not marking all touched files as "driver-resolved", to
  >     # emulate some practical use-cases where it is impossible to know the
  >     # file list before hand.
  > 
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels):
  >     ui.write("merge driver conclude\n")
  >     print_p1(ui, repo)
  > 
  >     # emulating an external script making changes to the working copy
  >     os.unlink("A")
  >     os.unlink("B_del")
  >     os.unlink("C_del")
  > 
  >     _ = open("B_add", "w").write("B")
  >     _ = open("C_add", "w").write("C")
  > 
  >     _ = open("B_change", "a").write("B")
  >     _ = open("C_change", "a").write("C")
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

  $ sl log -r . -T '{node}\n'
  07830bb2f52c641cbdf5980da2ed28d3e27810db
  $ sl graft $C
  grafting cb95dc195621 "C"
  merge driver preprocess
    dirstate p1: 07830bb2f52c641cbdf5980da2ed28d3e27810db
    repo['.']  : 07830bb2f52c641cbdf5980da2ed28d3e27810db
  merge driver conclude
    dirstate p1: 07830bb2f52c641cbdf5980da2ed28d3e27810db
    repo['.']  : 07830bb2f52c641cbdf5980da2ed28d3e27810db
  $ sl log -r . -T '{node}\n'
  787731c2a155fbee93b622a2ef0f20823e1e87e4

Status should be clean:

  $ sl status

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

  $ sl diff -r 'p1(.)' -r '.' --stat
   A        |  1 -
   B_add    |  1 +
   B_change |  2 +-
   B_del    |  1 -
   C        |  1 +
   C_add    |  1 +
   C_change |  1 +
   7 files changed, 5 insertions(+), 3 deletions(-)
