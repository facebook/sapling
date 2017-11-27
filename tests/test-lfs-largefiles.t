This tests the interaction between the largefiles and lfs extensions, and
conversion from largefiles -> lfs.

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > largefiles =
  > 
  > [lfs]
  > # standin files are 41 bytes.  Stay bigger for clarity.
  > threshold = 42
  > EOF

Setup a repo with a normal file and a largefile, above and below the lfs
threshold to test lfconvert.  *.txt start life as a normal file; *.bin start as
an lfs/largefile.

  $ hg init largefiles
  $ cd largefiles
  $ echo 'normal' > normal.txt
  $ echo 'normal above lfs threshold 0000000000000000000000000' > lfs.txt
  $ hg ci -Am 'normal.txt'
  adding lfs.txt
  adding normal.txt
  $ echo 'largefile' > large.bin
  $ echo 'largefile above lfs threshold 0000000000000000000000' > lfs.bin
  $ hg add --large large.bin lfs.bin
  $ hg ci -m 'add largefiles'

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs =
  > EOF

Add an lfs file and normal file that collide with files on the other branch.
large.bin is added as a normal file, and is named as such only to clash with the
largefile on the other branch.

  $ hg up -q '.^'
  $ echo 'below lfs threshold' > large.bin
  $ echo 'lfs above the lfs threshold for length 0000000000000' > lfs.bin
  $ hg ci -Am 'add with lfs extension'
  adding large.bin
  adding lfs.bin
  created new head

  $ hg log -G
  @  changeset:   2:e989d0fa3764
  |  tag:         tip
  |  parent:      0:29361292f54d
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add with lfs extension
  |
  | o  changeset:   1:6513aaab9ca0
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add largefiles
  |
  o  changeset:   0:29361292f54d
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     normal.txt
  
--------------------------------------------------------------------------------
Merge largefiles into lfs branch

The largefiles extension will prompt to use the normal or largefile when merged
into the lfs files.  `hg manifest` will show standins if present.  They aren't,
because largefiles merge doesn't merge content.  If it did, selecting (n)ormal
would convert to lfs on commit, if appropriate.

BUG: Largefiles isn't running the merge tool, like when two lfs files are
merged.  This is probably by design, but it should probably at least prompt if
content should be taken from (l)ocal or (o)ther as well.

  $ hg --config ui.interactive=True merge 6513aaab9ca0 <<EOF
  > n
  > n
  > EOF
  remote turned local normal file large.bin into a largefile
  use (l)argefile or keep (n)ormal file? n
  remote turned local normal file lfs.bin into a largefile
  use (l)argefile or keep (n)ormal file? n
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge lfs with largefiles -> normal'
  $ hg manifest
  large.bin
  lfs.bin
  lfs.txt
  normal.txt

The merged lfs.bin resolved to lfs because the (n)ormal option was picked.  The
lfs.txt file is unchanged by the merge, because it was added before lfs was
enabled, and the content didn't change.
  $ hg debugdata lfs.bin 0
  version https://git-lfs.github.com/spec/v1
  oid sha256:81c7492b2c05e130431f65a87651b54a30c5da72c99ce35a1e9b9872a807312b
  size 53
  x-is-binary 0
  $ hg debugdata lfs.txt 0
  normal above lfs threshold 0000000000000000000000000

Another filelog entry is NOT made by the merge, so nothing is committed as lfs.
  $ hg log -r . -T '{join(lfs_files, ", ")}\n'
  

Replay the last merge, but pick (l)arge this time.  The manifest will show any
standins.

  $ hg up -Cq e989d0fa3764

  $ hg --config ui.interactive=True merge 6513aaab9ca0 <<EOF
  > l
  > l
  > EOF
  remote turned local normal file large.bin into a largefile
  use (l)argefile or keep (n)ormal file? l
  remote turned local normal file lfs.bin into a largefile
  use (l)argefile or keep (n)ormal file? l
  getting changed largefiles
  2 largefiles updated, 0 removed
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge lfs with largefiles -> large'
  created new head
  $ hg manifest
  .hglf/large.bin
  .hglf/lfs.bin
  lfs.txt
  normal.txt

--------------------------------------------------------------------------------
Merge lfs into largefiles branch

  $ hg up -Cq 6513aaab9ca0
  $ hg --config ui.interactive=True merge e989d0fa3764 <<EOF
  > n
  > n
  > EOF
  remote turned local largefile large.bin into a normal file
  keep (l)argefile or use (n)ormal file? n
  remote turned local largefile lfs.bin into a normal file
  keep (l)argefile or use (n)ormal file? n
  getting changed largefiles
  0 largefiles updated, 0 removed
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge largefiles with lfs -> normal'
  created new head
  $ hg manifest
  large.bin
  lfs.bin
  lfs.txt
  normal.txt

The merged lfs.bin got converted to lfs because the (n)ormal option was picked.
The lfs.txt file is unchanged by the merge, because it was added before lfs was
enabled.
  $ hg debugdata lfs.bin 0
  version https://git-lfs.github.com/spec/v1
  oid sha256:81c7492b2c05e130431f65a87651b54a30c5da72c99ce35a1e9b9872a807312b
  size 53
  x-is-binary 0
  $ hg debugdata lfs.txt 0
  normal above lfs threshold 0000000000000000000000000

Another filelog entry is NOT made by the merge, so nothing is committed as lfs.
  $ hg log -r . -T '{join(lfs_files, ", ")}\n'
  

Replay the last merge, but pick (l)arge this time.  The manifest will show the
standins.

  $ hg up -Cq 6513aaab9ca0

  $ hg --config ui.interactive=True merge e989d0fa3764 <<EOF
  > l
  > l
  > EOF
  remote turned local largefile large.bin into a normal file
  keep (l)argefile or use (n)ormal file? l
  remote turned local largefile lfs.bin into a normal file
  keep (l)argefile or use (n)ormal file? l
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge largefiles with lfs -> large'
  created new head
  $ hg manifest
  .hglf/large.bin
  .hglf/lfs.bin
  lfs.txt
  normal.txt

--------------------------------------------------------------------------------

When both largefiles and lfs are configured to add by size, the tie goes to
largefiles since it hooks cmdutil.add() and lfs hooks the filelog write in the
commit.  By the time the commit occurs, the tracked file is smaller than the
threshold (assuming it is > 41, so the standins don't become lfs objects).

  $ $PYTHON -c 'import sys ; sys.stdout.write("y\n" * 1048576)' > large_by_size.bin
  $ hg --config largefiles.minsize=1 ci -Am 'large by size'
  adding large_by_size.bin as a largefile
  $ hg manifest
  .hglf/large.bin
  .hglf/large_by_size.bin
  .hglf/lfs.bin
  lfs.txt
  normal.txt

  $ hg rm large_by_size.bin
  $ hg ci -m 'remove large_by_size.bin'

Largefiles doesn't do anything special with diff, so it falls back to diffing
the standins.  Extdiff also is standin based comparison.  Diff and extdiff both
work on the original file for lfs objects.

Largefile -> lfs transition
  $ hg diff -r 1 -r 3
  diff -r 6513aaab9ca0 -r dcc5ce63e252 .hglf/large.bin
  --- a/.hglf/large.bin	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -cef9a458373df9b0743a0d3c14d0c66fb19b8629
  diff -r 6513aaab9ca0 -r dcc5ce63e252 .hglf/lfs.bin
  --- a/.hglf/lfs.bin	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -557fb6309cef935e1ac2c8296508379e4b15a6e6
  diff -r 6513aaab9ca0 -r dcc5ce63e252 large.bin
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/large.bin	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +below lfs threshold
  diff -r 6513aaab9ca0 -r dcc5ce63e252 lfs.bin
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/lfs.bin	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +lfs above the lfs threshold for length 0000000000000

lfs -> largefiles transition
  $ hg diff -r 2 -r 6
  diff -r e989d0fa3764 -r 95e1e80325c8 .hglf/large.bin
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/large.bin	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +cef9a458373df9b0743a0d3c14d0c66fb19b8629
  diff -r e989d0fa3764 -r 95e1e80325c8 .hglf/lfs.bin
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/lfs.bin	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +557fb6309cef935e1ac2c8296508379e4b15a6e6
  diff -r e989d0fa3764 -r 95e1e80325c8 large.bin
  --- a/large.bin	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -below lfs threshold
  diff -r e989d0fa3764 -r 95e1e80325c8 lfs.bin
  --- a/lfs.bin	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -lfs above the lfs threshold for length 0000000000000

A largefiles repo can be converted to lfs.  The lfconvert command uses the
convert extension under the hood with --to-normal.  So the --config based
parameters are available, but not --authormap, --branchmap, etc.

  $ cd ..
  $ hg lfconvert --to-normal largefiles nolargefiles 2>&1
  initializing destination nolargefiles
  0 additional largefiles cached
  scanning source...
  sorting...
  converting...
  8 normal.txt
  7 add largefiles
  6 add with lfs extension
  5 merge lfs with largefiles -> normal
  4 merge lfs with largefiles -> large
  3 merge largefiles with lfs -> normal
  2 merge largefiles with lfs -> large
  1 large by size
  0 remove large_by_size.bin
  $ cd nolargefiles

The requirement is added to the destination repo, and the extension is enabled
locally.

  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  lfs
  revlogv1
  store
  $ hg config --debug extensions | grep lfs
  $TESTTMP/nolargefiles/.hg/hgrc:*: extensions.lfs= (glob)

  $ hg log -r 'all()' -G -T '{rev} {join(lfs_files, ", ")} ({desc})\n'
  o  8  (remove large_by_size.bin)
  |
  o  7 large_by_size.bin (large by size)
  |
  o    6  (merge largefiles with lfs -> large)
  |\
  +---o  5  (merge largefiles with lfs -> normal)
  | |/
  +---o  4 lfs.bin (merge lfs with largefiles -> large)
  | |/
  +---o  3  (merge lfs with largefiles -> normal)
  | |/
  | o  2 lfs.bin (add with lfs extension)
  | |
  o |  1 lfs.bin (add largefiles)
  |/
  o  0 lfs.txt (normal.txt)
  
  $ hg debugdata lfs.bin 0
  version https://git-lfs.github.com/spec/v1
  oid sha256:2172a5bd492dd41ec533b9bb695f7691b6351719407ac797f0ccad5348c81e62
  size 53
  x-is-binary 0
  $ hg debugdata lfs.bin 1
  version https://git-lfs.github.com/spec/v1
  oid sha256:81c7492b2c05e130431f65a87651b54a30c5da72c99ce35a1e9b9872a807312b
  size 53
  x-is-binary 0
  $ hg debugdata lfs.bin 2
  version https://git-lfs.github.com/spec/v1
  oid sha256:2172a5bd492dd41ec533b9bb695f7691b6351719407ac797f0ccad5348c81e62
  size 53
  x-is-binary 0
  $ hg debugdata lfs.bin 3
  abort: invalid revision identifier 3
  [255]

No diffs when comparing merge and p1 that kept p1's changes.  Diff of lfs to
largefiles no longer operates in standin files.

  $ hg diff -r 2:3
  $ hg diff -r 2:6
  diff -r e989d0fa3764 -r 752e3a0d8488 large.bin
  --- a/large.bin	Thu Jan 01 00:00:00 1970 +0000
  +++ b/large.bin	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -below lfs threshold
  +largefile
  diff -r e989d0fa3764 -r 752e3a0d8488 lfs.bin
  --- a/lfs.bin	Thu Jan 01 00:00:00 1970 +0000
  +++ b/lfs.bin	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -lfs above the lfs threshold for length 0000000000000
  +largefile above lfs threshold 0000000000000000000000
