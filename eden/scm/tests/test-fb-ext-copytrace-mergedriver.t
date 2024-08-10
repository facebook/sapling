
#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# With copied file using the heuristics copytracing:

  $ eagerepo

  $ enable mergedriver

  $ newrepo
  $ enable amend
  $ setconfig 'experimental.mergedriver=python:$TESTTMP/m.py'

  $ drawdag << 'EOS'
  > B C
  > |/
  > A
  > |
  > Z
  > EOS

  $ cat > $TESTTMP/m.py << 'EOF'
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels):
  >     ui.write("unresolved: %r\n" % (sorted(mergestate.unresolved())))
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels):
  >     pass
  > EOF

  $ hg up -q $B

#  (trigger amend copytrace code path)

  $ hg cp A D
  $ hg cp A E
  $ hg amend -m B2 -d '0 0'
  $ hg bookmark -i book-B

# Do the merge:

  $ hg up -q $C
  $ hg graft book-B
  grafting b55db8435dc2 "B2" (book-B)

  $ hg status

Be sure to record copy metadata.
  $ hg log -r . -p --config diff.git=true
  commit:      599c51a4e5d9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B2
  
  diff --git a/B b/B
  new file mode 100644
  --- /dev/null
  +++ b/B
  @@ -0,0 +1,1 @@
  +B
  \ No newline at end of file
  diff --git a/A b/D
  copy from A
  copy to D
  diff --git a/A b/E
  copy from A
  copy to E

# Run again with dagcopytrace disabled:

  $ setconfig copytrace.dagcopytrace=False

  $ hg up -q $C
  $ hg graft book-B
  grafting b55db8435dc2 "B2" (book-B)
