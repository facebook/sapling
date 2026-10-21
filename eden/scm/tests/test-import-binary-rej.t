#require no-eden

Rejecting a GIT binary hunk crashes `hg import` with a TypeError instead of
writing the .rej file (binhunk's "GIT binary patch" header is a str; every
other line in `lines` is bytes, and the .rej is opened as a binary file).
Traceback line numbers vary, so only the stable lines are asserted.

  $ newclientrepo
  $ echo text > text
  $ hg ci -Aqm text
  $ printf '\0\1\2\3binary\n' > bin
  $ hg ci -Aqm bin
  $ hg export --git -r . > $TESTTMP/bin.patch
  $ grep -A 3 'GIT binary' $TESTTMP/bin.patch
  GIT binary patch
  literal 14
  Vc${O1F^n;aF;2?NODwA70stK11Udi!

The patch creates "bin", but an untracked "bin" already exists: the binary
hunk is rejected. The process crashes right after announcing the .rej, and
the .rej is left behind empty.

  $ hg up -q 'desc(text)'
  $ printf 'other' > bin
  $ hg import --no-commit $TESTTMP/bin.patch > $TESTTMP/out 2>&1
  [1]
  $ grep -E '^(applying|file bin|1 out of|\*\* Sapling|TypeError)' $TESTTMP/out
  applying $TESTTMP/bin.patch
  file bin already exists
  1 out of 1 hunks FAILED -- saving rejects to file bin.rej
  ** Sapling SCM (version *) has crashed: (glob)
  TypeError: a bytes-like object is required, not 'str'
  $ wc -c < bin.rej
  0
  $ cat bin
  other (no-eol)

Same with --partial.

  $ rm bin.rej
  $ hg import --partial $TESTTMP/bin.patch > $TESTTMP/out 2>&1
  [1]
  $ grep -E '^(applying|file bin|1 out of|\*\* Sapling|TypeError|patch applied)' $TESTTMP/out
  applying $TESTTMP/bin.patch
  file bin already exists
  1 out of 1 hunks FAILED -- saving rejects to file bin.rej
  ** Sapling SCM (version *) has crashed: (glob)
  TypeError: a bytes-like object is required, not 'str'
  $ wc -c < bin.rej
  0

The other reject path for a binary hunk: the target file is missing (-p 2
strips the directory, so the patch names a file that does not exist).

  $ newclientrepo
  $ mkdir dir
  $ printf '\0\1\2\3binary\n' > dir/bin
  $ hg ci -Aqm bin
  $ printf '\0\1\2\3changed\n' > dir/bin
  $ hg ci -qm change
  $ hg export --git -r . > $TESTTMP/change.patch
  $ hg up -q 'desc(bin)'
  $ hg import --no-commit -p 2 $TESTTMP/change.patch > $TESTTMP/out 2>&1
  [1]
  $ grep -E "^(applying|unable to find|1 out of|\*\* Sapling|TypeError)" $TESTTMP/out
  applying $TESTTMP/change.patch
  unable to find 'bin' for patching
  1 out of 1 hunks FAILED -- saving rejects to file bin.rej
  ** Sapling SCM (version *) has crashed: (glob)
  TypeError: a bytes-like object is required, not 'str'
  $ wc -c < bin.rej
  0
