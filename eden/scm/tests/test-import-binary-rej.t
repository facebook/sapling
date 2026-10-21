#require no-eden

A rejected GIT binary hunk must be saved to the .rej file, like a text hunk.

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
hunk is rejected and written to bin.rej.

  $ hg up -q 'desc(text)'
  $ printf 'other' > bin
  $ hg import --no-commit $TESTTMP/bin.patch
  applying $TESTTMP/bin.patch
  file bin already exists
  1 out of 1 hunks FAILED -- saving rejects to file bin.rej
  abort: patch failed to apply
  [255]
  $ cat bin.rej
  --- bin
  +++ bin
  GIT binary patch
  literal 14
  Vc${O1F^n;aF;2?NODwA70stK11Udi!
  $ cat bin
  other (no-eol)

The .rej is a valid patch again: once the conflicting file is gone it applies.

  $ rm bin bin.rej
  $ hg import --no-commit $TESTTMP/bin.patch
  applying $TESTTMP/bin.patch
  $ hg st
  A bin

Same with --partial: exit 1, partial commit, bin.rej written.

  $ hg revert -qa
  $ rm bin
  $ printf 'other' > bin
  $ hg import --partial $TESTTMP/bin.patch
  applying $TESTTMP/bin.patch
  file bin already exists
  1 out of 1 hunks FAILED -- saving rejects to file bin.rej
  patch applied partially
  (fix the .rej files and run `hg commit --amend`)
  [1]
  $ cat bin.rej
  --- bin
  +++ bin
  GIT binary patch
  literal 14
  Vc${O1F^n;aF;2?NODwA70stK11Udi!

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
  $ hg import --no-commit -p 2 $TESTTMP/change.patch
  applying $TESTTMP/change.patch
  unable to find 'bin' for patching
  (use '--prefix' to apply patch relative to the current directory)
  1 out of 1 hunks FAILED -- saving rejects to file bin.rej
  abort: patch failed to apply
  [255]
  $ cat bin.rej
  --- bin
  +++ bin
  GIT binary patch
  literal 15
  Wc${O1F^n;aF;31%%u7#A;Q|05W(0o#
