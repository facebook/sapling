test merging things outside of the sparse checkout

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=
  > EOF

  $ echo foo > foo
  $ echo bar > bar
  $ hg add foo bar
  $ hg commit -m initial

  $ hg branch feature
  marked working directory as branch feature
  (branches are permanent and global, did you want a bookmark?)
  $ echo bar2 >> bar
  $ hg commit -m 'feature - bar2'

  $ hg update -q default
  $ hg debugsparse --exclude 'bar**'

  $ hg merge feature
  temporarily included 1 file(s) in the sparse checkout for merging
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
  $ hg status
  $ ls
  foo

  $ hg cat -r . bar
  bar
  bar2

Test merging things outside of the sparse checkout that are not in the working
copy

  $ hg strip -q -r . --config extensions.strip=
  $ hg up -q feature
  $ touch branchonly
  $ hg ci -Aqm 'add branchonly'

  $ hg up -q default
  $ hg debugsparse -X branchonly
  $ hg merge feature
  temporarily included 2 file(s) in the sparse checkout for merging
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
