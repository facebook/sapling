  $ hg init repo
  $ cd repo

  $ echo line 1 > foo
  $ hg ci -qAm 'add foo'

copy foo to bar and change both files
  $ hg cp foo bar
  $ echo line 2-1 >> foo
  $ echo line 2-2 >> bar
  $ hg ci -m 'cp foo bar; change both'

in another branch, change foo in a way that doesn't conflict with
the other changes
  $ hg up -qC 0
  $ echo line 0 > foo
  $ hg cat foo >> foo
  $ hg ci -m 'change foo'
  created new head

we get conflicts that shouldn't be there
  $ hg merge -P
  changeset:   1:484bf6903104
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     cp foo bar; change both
  
  $ hg merge --debug
    searching for copies back to rev 1
    unmatched files in other:
     bar
    all copies found (* = to merge, ! = divergent):
     bar -> foo *
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor e6dc8efe11cc local 6a0df1dad128+ remote 484bf6903104
   foo: versions differ -> m
   foo: remote copied to bar -> m
  preserving foo for resolve of bar
  preserving foo for resolve of foo
  updating: foo 1/2 files (50.00%)
  picked tool 'internal:merge' for bar (binary False symlink False)
  merging foo and bar to bar
  my bar@6a0df1dad128+ other bar@484bf6903104 ancestor foo@e6dc8efe11cc
   premerge successful
  updating: foo 2/2 files (100.00%)
  picked tool 'internal:merge' for foo (binary False symlink False)
  merging foo
  my foo@6a0df1dad128+ other foo@484bf6903104 ancestor foo@e6dc8efe11cc
   premerge successful
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

contents of foo
  $ cat foo
  line 0
  line 1
  line 2-1

contents of bar
  $ cat bar
  line 0
  line 1
  line 2-2
