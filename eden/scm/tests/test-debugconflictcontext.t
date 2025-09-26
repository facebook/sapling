  $ enable rebase

  $ newclientrepo

Set things up where $D will conflict rebasing onto $C due to a change introduced by $B.
$C also changes the file, but in a mergable way.
  $ drawdag <<EOS
  > C    # C/foo = conflict\ntwo\nokay\n
  > |    # D/foo = change\ntwo\nthree\n
  > B D  # B/foo = conflict\ntwo\nthree\n
  > |/   # B/bar = some other really big change
  > A    # A/foo = one\ntwo\nthree\n
  > # drawdag.defaultfiles=false
  > EOS

  $ hg rebase -r $D -d $C
  rebasing ab91ba867301 "D"
  merging foo
  warning: 1 conflicts while merging foo! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

We correctly found "B" to be the commit that likely introduced the conflict:
  $ hg debugconflictcontext | pp
  [
    {
      "conflicting_local": {
        "description": "B",
        "diff": "diff -r a18912b06f5d -r 8c66ef252ef2 B\n--- /dev/null\tThu Jan 01 00:00:00 1970 +0000\n+++ b/B\tThu Jan 01 00:00:00 1970 +0000\n@@ -0,0 +1,1 @@\n+B\n\\ No newline at end of file\ndiff -r a18912b06f5d -r 8c66ef252ef2 bar\n--- /dev/null\tThu Jan 01 00:00:00 1970 +0000\n+++ b/bar\tThu Jan 01 00:00:00 1970 +0000\n@@ -0,0 +1,1 @@\n+some other really big change\n\\ No newline at end of file\ndiff -r a18912b06f5d -r 8c66ef252ef2 foo\n--- a/foo\tThu Jan 01 00:00:00 1970 +0000\n+++ b/foo\tThu Jan 01 00:00:00 1970 +0000\n@@ -1,3 +1,3 @@\n-one\n+conflict\n two\n three\n",
        "hash": "8c66ef252ef2b17992b938ee6ebb9154231e7a8c"
      },
      "conflicting_other": {
        "description": "D",
        "diff": "diff -r a18912b06f5d -r ab91ba867301 D\n--- /dev/null\tThu Jan 01 00:00:00 1970 +0000\n+++ b/D\tThu Jan 01 00:00:00 1970 +0000\n@@ -0,0 +1,1 @@\n+D\n\\ No newline at end of file\ndiff -r a18912b06f5d -r ab91ba867301 foo\n--- a/foo\tThu Jan 01 00:00:00 1970 +0000\n+++ b/foo\tThu Jan 01 00:00:00 1970 +0000\n@@ -1,3 +1,3 @@\n-one\n+change\n two\n three\n",
        "hash": "ab91ba867301844de735473576e310f2e892b428"
      },
      "file": "foo"
    }
  ]

If full diff is too large, limit to just the particular file's diff (i.e. exclude the "bar" file):
  $ hg debugconflictcontext --max-diff-size=512 | pp
  [
    {
      "conflicting_local": {
        "description": "B",
        "diff": "diff -r a18912b06f5d -r 8c66ef252ef2 foo\n--- a/foo\tThu Jan 01 00:00:00 1970 +0000\n+++ b/foo\tThu Jan 01 00:00:00 1970 +0000\n@@ -1,3 +1,3 @@\n-one\n+conflict\n two\n three\n",
        "hash": "8c66ef252ef2b17992b938ee6ebb9154231e7a8c"
      },
      "conflicting_other": {
        "description": "D",
        "diff": "diff -r a18912b06f5d -r ab91ba867301 D\n--- /dev/null\tThu Jan 01 00:00:00 1970 +0000\n+++ b/D\tThu Jan 01 00:00:00 1970 +0000\n@@ -0,0 +1,1 @@\n+D\n\\ No newline at end of file\ndiff -r a18912b06f5d -r ab91ba867301 foo\n--- a/foo\tThu Jan 01 00:00:00 1970 +0000\n+++ b/foo\tThu Jan 01 00:00:00 1970 +0000\n@@ -1,3 +1,3 @@\n-one\n+change\n two\n three\n",
        "hash": "ab91ba867301844de735473576e310f2e892b428"
      },
      "file": "foo"
    }
  ]
