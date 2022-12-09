#chg-compatible
#require no-windows
#debugruntest-compatible
#inprocess-hg-incompatible

test merging things outside of the sparse checkout

  $ hg init myrepo
  $ cd myrepo
  $ enable sparse

  $ echo foo > foo
  $ hg commit -m initial -A foo
  $ hg bookmark -ir. initial

  $ echo bar > bar
  $ hg commit -m 'feature - bar1' -A bar
  $ hg bookmark -ir. feature1

  $ hg goto --inactive -q initial
  $ echo bar2 > bar
  $ hg commit -m 'feature - bar2' -A bar
  $ hg bookmark -ir. feature2

  $ hg goto --inactive -q feature1
  $ hg sparse --exclude 'bar*'

  $ hg merge feature2 --tool :merge-other
  temporarily included 1 file(s) in the sparse checkout for merging
  merging bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Verify bar was merged temporarily

  $ ls
  bar
  foo
  $ hg status
  M bar

Verify bar disappears automatically when the working copy becomes clean

  $ hg commit -m "merged"
  cleaned up 1 temporarily added file(s) from the sparse checkout
  $ hg bookmark -ir. merged
  $ hg status
  $ ls
  foo

  $ hg cat -r . bar
  bar2

Test merging things outside of the sparse checkout that are not in the working
copy

  $ hg debugstrip -q -r .
  $ hg up --inactive -q feature2
  $ touch branchonly
  $ hg ci -Aqm 'add branchonly'

  $ hg up --inactive -q feature1
  $ hg sparse -X branchonly
  $ hg merge feature2 --tool :merge-other
  temporarily included 1 file(s) in the sparse checkout for merging
  merging bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
