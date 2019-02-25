With copied file using the heuristics copytracing:

  $ enable mergedriver

  $ newrepo
  $ enable copytrace amend
  $ setconfig copytrace.draftusefullcopytrace=0 experimental.copytrace=off copytrace.fastcopytrace=1 experimental.mergedriver=python:$TESTTMP/m.py

  $ drawdag << 'EOS'
  > B C
  > |/
  > A
  > |
  > Z
  > EOS

  $ cat > $TESTTMP/m.py << EOF
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels):
  >     ui.write("unresolved: %r\n" % (sorted(mergestate.unresolved())))
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels):
  >     pass
  > EOF

  $ hg up -q $B
 (trigger amend copytrace code path)
  $ hg cp A D
  $ hg cp A E
  $ hg amend -m B2 -d '0 0'
  $ hg bookmark -i book-B

Do the merge:

  $ hg up -q $C
  $ hg graft book-B
  grafting 4:b55db8435dc2 "B2" (tip book-B)
  unresolved: ['D', 'E']

BUG: D and E should not be unresolved.

  $ hg status
  $ hg log -r . -T '{desc}\n' --stat
  B2
   B |  1 +
   D |  1 +
   E |  1 +
   3 files changed, 3 insertions(+), 0 deletions(-)
  

Run again with heuristics copytrace disabled:

  $ setconfig extensions.copytrace=! experimental.copytrace=on copytrace.fastcopytrace=0

  $ hg up -q $C
  $ hg graft book-B
  grafting 4:b55db8435dc2 "B2" (book-B)

