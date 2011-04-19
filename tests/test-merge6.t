  $ cat <<EOF > merge
  > import sys, os
  > print "merging for", os.path.basename(sys.argv[1])
  > EOF
  $ HGMERGE="python ../merge"; export HGMERGE

  $ hg init A1
  $ cd A1
  $ echo This is file foo1 > foo
  $ echo This is file bar1 > bar
  $ hg add foo bar
  $ hg commit -m "commit text"

  $ cd ..
  $ hg clone A1 B1
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd A1
  $ rm bar
  $ hg remove bar
  $ hg commit -m "commit test"

  $ cd ../B1
  $ echo This is file foo22 > foo
  $ hg commit -m "commit test"

  $ cd ..
  $ hg clone A1 A2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone B1 B2
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd A1
  $ hg pull ../B1
  pulling from ../B1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "commit test"
bar should remain deleted.
  $ hg manifest --debug
  f9b0e817f6a48de3564c6b2957687c5e7297c5a0 644   foo

  $ cd ../B2
  $ hg pull ../A2
  pulling from ../A2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg merge
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "commit test"
bar should remain deleted.
  $ hg manifest --debug
  f9b0e817f6a48de3564c6b2957687c5e7297c5a0 644   foo
