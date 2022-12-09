#chg-compatible

  $ setconfig workingcopy.ruststatus=False
  $ setconfig status.use-rust=False workingcopy.use-rust=False

Set up a base, local, and remote changeset, as well as the working copy state.
Files names are of the form base_remote_local_working-copy. For example,
content1_content2_content1_content2-untracked represents a
file that was modified in the remote changeset, left untouched in the
local changeset, and then modified in the working copy to match the
remote content, then finally forgotten.

  $ hg init repo
  $ cd repo

Create base changeset

  $ $PYTHON $TESTDIR/generateworkingcopystates.py state 3 1
  $ hg addremove -q --similarity 0
  $ hg commit -qm 'base'

Create remote changeset

  $ $PYTHON $TESTDIR/generateworkingcopystates.py state 3 2
  $ hg addremove -q --similarity 0
  $ hg commit -qm 'remote'

Create local changeset

  $ hg goto -q 'desc(base)'
  $ $PYTHON $TESTDIR/generateworkingcopystates.py state 3 3
  $ hg addremove -q --similarity 0
  $ hg commit -qm 'local'

Set up working directory

  $ $PYTHON $TESTDIR/generateworkingcopystates.py state 3 wc
  $ hg addremove -q --similarity 0
  $ hg forget *_*_*_*-untracked
  $ rm *_*_*_missing-*

  $ hg status -A
  M content1_content1_content1_content4-tracked
  M content1_content1_content3_content1-tracked
  M content1_content1_content3_content4-tracked
  M content1_content2_content1_content2-tracked
  M content1_content2_content1_content4-tracked
  M content1_content2_content2_content1-tracked
  M content1_content2_content2_content4-tracked
  M content1_content2_content3_content1-tracked
  M content1_content2_content3_content2-tracked
  M content1_content2_content3_content4-tracked
  M content1_missing_content1_content4-tracked
  M content1_missing_content3_content1-tracked
  M content1_missing_content3_content4-tracked
  M missing_content2_content2_content4-tracked
  M missing_content2_content3_content2-tracked
  M missing_content2_content3_content4-tracked
  M missing_missing_content3_content4-tracked
  A content1_content1_missing_content1-tracked
  A content1_content1_missing_content4-tracked
  A content1_content2_missing_content1-tracked
  A content1_content2_missing_content2-tracked
  A content1_content2_missing_content4-tracked
  A content1_missing_missing_content1-tracked
  A content1_missing_missing_content4-tracked
  A missing_content2_missing_content2-tracked
  A missing_content2_missing_content4-tracked
  A missing_missing_missing_content4-tracked
  R content1_content1_content1_content1-untracked
  R content1_content1_content1_content4-untracked
  R content1_content1_content1_missing-untracked
  R content1_content1_content3_content1-untracked
  R content1_content1_content3_content3-untracked
  R content1_content1_content3_content4-untracked
  R content1_content1_content3_missing-untracked
  R content1_content2_content1_content1-untracked
  R content1_content2_content1_content2-untracked
  R content1_content2_content1_content4-untracked
  R content1_content2_content1_missing-untracked
  R content1_content2_content2_content1-untracked
  R content1_content2_content2_content2-untracked
  R content1_content2_content2_content4-untracked
  R content1_content2_content2_missing-untracked
  R content1_content2_content3_content1-untracked
  R content1_content2_content3_content2-untracked
  R content1_content2_content3_content3-untracked
  R content1_content2_content3_content4-untracked
  R content1_content2_content3_missing-untracked
  R content1_missing_content1_content1-untracked
  R content1_missing_content1_content4-untracked
  R content1_missing_content1_missing-untracked
  R content1_missing_content3_content1-untracked
  R content1_missing_content3_content3-untracked
  R content1_missing_content3_content4-untracked
  R content1_missing_content3_missing-untracked
  R missing_content2_content2_content2-untracked
  R missing_content2_content2_content4-untracked
  R missing_content2_content2_missing-untracked
  R missing_content2_content3_content2-untracked
  R missing_content2_content3_content3-untracked
  R missing_content2_content3_content4-untracked
  R missing_content2_content3_missing-untracked
  R missing_missing_content3_content3-untracked
  R missing_missing_content3_content4-untracked
  R missing_missing_content3_missing-untracked
  ! content1_content1_content1_missing-tracked
  ! content1_content1_content3_missing-tracked
  ! content1_content1_missing_missing-tracked
  ! content1_content2_content1_missing-tracked
  ! content1_content2_content2_missing-tracked
  ! content1_content2_content3_missing-tracked
  ! content1_content2_missing_missing-tracked
  ! content1_missing_content1_missing-tracked
  ! content1_missing_content3_missing-tracked
  ! content1_missing_missing_missing-tracked
  ! missing_content2_content2_missing-tracked
  ! missing_content2_content3_missing-tracked
  ! missing_content2_missing_missing-tracked
  ! missing_missing_content3_missing-tracked
  ! missing_missing_missing_missing-tracked
  ? content1_content1_missing_content1-untracked
  ? content1_content1_missing_content4-untracked
  ? content1_content2_missing_content1-untracked
  ? content1_content2_missing_content2-untracked
  ? content1_content2_missing_content4-untracked
  ? content1_missing_missing_content1-untracked
  ? content1_missing_missing_content4-untracked
  ? missing_content2_missing_content2-untracked
  ? missing_content2_missing_content4-untracked
  ? missing_missing_missing_content4-untracked
  C content1_content1_content1_content1-tracked
  C content1_content1_content3_content3-tracked
  C content1_content2_content1_content1-tracked
  C content1_content2_content2_content2-tracked
  C content1_content2_content3_content3-tracked
  C content1_missing_content1_content1-tracked
  C content1_missing_content3_content3-tracked
  C missing_content2_content2_content2-tracked
  C missing_content2_content3_content3-tracked
  C missing_missing_content3_content3-tracked

Merge with remote

# Notes:
# - local and remote changed content1_content2_*_content2-untracked
#   in the same way, so it could potentially be left alone

  $ hg merge -f --tool internal:merge3 'desc("remote")' 2>&1 | tee $TESTTMP/merge-output-1
  local [working copy] changed content1_missing_content1_content4-tracked which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  local [working copy] changed content1_missing_content3_content3-tracked which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  local [working copy] changed content1_missing_content3_content4-tracked which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  local [working copy] changed content1_missing_missing_content4-tracked which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  other [merge rev] changed content1_content2_content1_content1-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content1_content2-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content1_content4-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content1_missing-tracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content1_missing-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content2_content1-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content2_content2-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content2_content4-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content2_missing-tracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content2_missing-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content3_content1-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content3_content2-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content3_content3-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content3_content4-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content3_missing-tracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content3_missing-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_missing_content1-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_missing_content2-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_missing_content4-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_missing_missing-tracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_missing_missing-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  merging content1_content2_content1_content4-tracked
  merging content1_content2_content2_content1-tracked
  merging content1_content2_content2_content4-tracked
  merging content1_content2_content3_content1-tracked
  merging content1_content2_content3_content3-tracked
  merging content1_content2_content3_content4-tracked
  merging content1_content2_missing_content1-tracked
  merging content1_content2_missing_content4-tracked
  merging missing_content2_content2_content4-tracked
  merging missing_content2_content3_content3-tracked
  merging missing_content2_content3_content4-tracked
  merging missing_content2_missing_content4-tracked
  merging missing_content2_missing_content4-untracked
  warning: 1 conflicts while merging content1_content2_content1_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging content1_content2_content2_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging content1_content2_content3_content3-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging content1_content2_content3_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging content1_content2_missing_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging missing_content2_content2_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging missing_content2_content3_content3-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging missing_content2_content3_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging missing_content2_missing_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging missing_content2_missing_content4-untracked! (edit, then use 'hg resolve --mark')
  18 files updated, 3 files merged, 8 files removed, 35 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon

Check which files need to be resolved (should correspond to the output above).
This should be the files for which the base (1st filename segment), the remote
(2nd segment) and the working copy (4th segment) are all different.

Interestingly, one untracked file got merged and added, which corresponds to the
odd 'if force and branchmerge and different' case in manifestmerge().

  $ hg resolve -l
  U content1_content2_content1_content1-untracked
  U content1_content2_content1_content2-untracked
  U content1_content2_content1_content4-tracked
  U content1_content2_content1_content4-untracked
  U content1_content2_content1_missing-tracked
  U content1_content2_content1_missing-untracked
  R content1_content2_content2_content1-tracked
  U content1_content2_content2_content1-untracked
  U content1_content2_content2_content2-untracked
  U content1_content2_content2_content4-tracked
  U content1_content2_content2_content4-untracked
  U content1_content2_content2_missing-tracked
  U content1_content2_content2_missing-untracked
  R content1_content2_content3_content1-tracked
  U content1_content2_content3_content1-untracked
  U content1_content2_content3_content2-untracked
  U content1_content2_content3_content3-tracked
  U content1_content2_content3_content3-untracked
  U content1_content2_content3_content4-tracked
  U content1_content2_content3_content4-untracked
  U content1_content2_content3_missing-tracked
  U content1_content2_content3_missing-untracked
  R content1_content2_missing_content1-tracked
  U content1_content2_missing_content1-untracked
  U content1_content2_missing_content2-untracked
  U content1_content2_missing_content4-tracked
  U content1_content2_missing_content4-untracked
  U content1_content2_missing_missing-tracked
  U content1_content2_missing_missing-untracked
  U content1_missing_content1_content4-tracked
  U content1_missing_content3_content3-tracked
  U content1_missing_content3_content4-tracked
  U content1_missing_missing_content4-tracked
  U missing_content2_content2_content4-tracked
  U missing_content2_content3_content3-tracked
  U missing_content2_content3_content4-tracked
  U missing_content2_missing_content4-tracked
  U missing_content2_missing_content4-untracked

Check status and file content

Some files get added (e.g. content1_content2_content1_content1-untracked)

It is not intuitive that content1_content2_content1_content4-tracked gets
merged while content1_content2_content1_content4-untracked gets overwritten.
Any *_content2_*-untracked triggers the modified/deleted prompt and then gets
overwritten.

A lot of untracked files become tracked, for example
content1_content2_content2_content2-untracked.

missing_missing_content3_missing-tracked becomes removed ('R'), even though
the remote side did not touch the file

  $ checkstatus() {
  >   for f in `$PYTHON $TESTDIR/generateworkingcopystates.py filelist 3`
  >   do
  >     echo
  >     hg status -A $f
  >     if test -f $f
  >     then
  >       cat $f
  >     else
  >       echo '<missing>'
  >     fi
  >   done
  > }
  $ checkstatus 2>&1 | tee $TESTTMP/status1
  
  C content1_content1_content1_content1-tracked
  content1
  
  R content1_content1_content1_content1-untracked
  content1
  
  M content1_content1_content1_content4-tracked
  content4
  
  R content1_content1_content1_content4-untracked
  content4
  
  ! content1_content1_content1_missing-tracked
  <missing>
  
  R content1_content1_content1_missing-untracked
  <missing>
  
  M content1_content1_content3_content1-tracked
  content1
  
  R content1_content1_content3_content1-untracked
  content1
  
  C content1_content1_content3_content3-tracked
  content3
  
  R content1_content1_content3_content3-untracked
  content3
  
  M content1_content1_content3_content4-tracked
  content4
  
  R content1_content1_content3_content4-untracked
  content4
  
  ! content1_content1_content3_missing-tracked
  <missing>
  
  R content1_content1_content3_missing-untracked
  <missing>
  
  A content1_content1_missing_content1-tracked
  content1
  
  ? content1_content1_missing_content1-untracked
  content1
  
  A content1_content1_missing_content4-tracked
  content4
  
  ? content1_content1_missing_content4-untracked
  content4
  
  ! content1_content1_missing_missing-tracked
  <missing>
  
  content1_content1_missing_missing-untracked: * (glob)
  <missing>
  
  M content1_content2_content1_content1-tracked
  content2
  
  M content1_content2_content1_content1-untracked
  content2
  
  M content1_content2_content1_content2-tracked
  content2
  
  M content1_content2_content1_content2-untracked
  content2
  
  M content1_content2_content1_content4-tracked
  <<<<<<< working copy: 0447570f1af6 - test: local
  content4
  ||||||| base
  content1
  =======
  content2
  >>>>>>> merge rev:    85100b8c675b - test: remote
  
  M content1_content2_content1_content4-untracked
  content2
  
  M content1_content2_content1_missing-tracked
  content2
  
  M content1_content2_content1_missing-untracked
  content2
  
  M content1_content2_content2_content1-tracked
  content2
  
  M content1_content2_content2_content1-untracked
  content2
  
  C content1_content2_content2_content2-tracked
  content2
  
  M content1_content2_content2_content2-untracked
  content2
  
  M content1_content2_content2_content4-tracked
  <<<<<<< working copy: 0447570f1af6 - test: local
  content4
  ||||||| base
  content1
  =======
  content2
  >>>>>>> merge rev:    85100b8c675b - test: remote
  
  M content1_content2_content2_content4-untracked
  content2
  
  M content1_content2_content2_missing-tracked
  content2
  
  M content1_content2_content2_missing-untracked
  content2
  
  M content1_content2_content3_content1-tracked
  content2
  
  M content1_content2_content3_content1-untracked
  content2
  
  M content1_content2_content3_content2-tracked
  content2
  
  M content1_content2_content3_content2-untracked
  content2
  
  M content1_content2_content3_content3-tracked
  <<<<<<< working copy: 0447570f1af6 - test: local
  content3
  ||||||| base
  content1
  =======
  content2
  >>>>>>> merge rev:    85100b8c675b - test: remote
  
  M content1_content2_content3_content3-untracked
  content2
  
  M content1_content2_content3_content4-tracked
  <<<<<<< working copy: 0447570f1af6 - test: local
  content4
  ||||||| base
  content1
  =======
  content2
  >>>>>>> merge rev:    85100b8c675b - test: remote
  
  M content1_content2_content3_content4-untracked
  content2
  
  M content1_content2_content3_missing-tracked
  content2
  
  M content1_content2_content3_missing-untracked
  content2
  
  M content1_content2_missing_content1-tracked
  content2
  
  M content1_content2_missing_content1-untracked
  content2
  
  M content1_content2_missing_content2-tracked
  content2
  
  M content1_content2_missing_content2-untracked
  content2
  
  M content1_content2_missing_content4-tracked
  <<<<<<< working copy: 0447570f1af6 - test: local
  content4
  ||||||| base
  content1
  =======
  content2
  >>>>>>> merge rev:    85100b8c675b - test: remote
  
  M content1_content2_missing_content4-untracked
  content2
  
  M content1_content2_missing_missing-tracked
  content2
  
  M content1_content2_missing_missing-untracked
  content2
  
  R content1_missing_content1_content1-tracked
  <missing>
  
  R content1_missing_content1_content1-untracked
  content1
  
  M content1_missing_content1_content4-tracked
  content4
  
  R content1_missing_content1_content4-untracked
  content4
  
  R content1_missing_content1_missing-tracked
  <missing>
  
  R content1_missing_content1_missing-untracked
  <missing>
  
  R content1_missing_content3_content1-tracked
  <missing>
  
  R content1_missing_content3_content1-untracked
  content1
  
  C content1_missing_content3_content3-tracked
  content3
  
  R content1_missing_content3_content3-untracked
  content3
  
  M content1_missing_content3_content4-tracked
  content4
  
  R content1_missing_content3_content4-untracked
  content4
  
  R content1_missing_content3_missing-tracked
  <missing>
  
  R content1_missing_content3_missing-untracked
  <missing>
  
  content1_missing_missing_content1-tracked: $ENOENT$
  <missing>
  
  ? content1_missing_missing_content1-untracked
  content1
  
  A content1_missing_missing_content4-tracked
  content4
  
  ? content1_missing_missing_content4-untracked
  content4
  
  content1_missing_missing_missing-tracked: $ENOENT$
  <missing>
  
  content1_missing_missing_missing-untracked: * (glob)
  <missing>
  
  C missing_content2_content2_content2-tracked
  content2
  
  M missing_content2_content2_content2-untracked
  content2
  
  M missing_content2_content2_content4-tracked
  <<<<<<< working copy: 0447570f1af6 - test: local
  content4
  ||||||| base
  =======
  content2
  >>>>>>> merge rev:    85100b8c675b - test: remote
  
  M missing_content2_content2_content4-untracked
  content2
  
  M missing_content2_content2_missing-tracked
  content2
  
  M missing_content2_content2_missing-untracked
  content2
  
  M missing_content2_content3_content2-tracked
  content2
  
  M missing_content2_content3_content2-untracked
  content2
  
  M missing_content2_content3_content3-tracked
  <<<<<<< working copy: 0447570f1af6 - test: local
  content3
  ||||||| base
  =======
  content2
  >>>>>>> merge rev:    85100b8c675b - test: remote
  
  M missing_content2_content3_content3-untracked
  content2
  
  M missing_content2_content3_content4-tracked
  <<<<<<< working copy: 0447570f1af6 - test: local
  content4
  ||||||| base
  =======
  content2
  >>>>>>> merge rev:    85100b8c675b - test: remote
  
  M missing_content2_content3_content4-untracked
  content2
  
  M missing_content2_content3_missing-tracked
  content2
  
  M missing_content2_content3_missing-untracked
  content2
  
  M missing_content2_missing_content2-tracked
  content2
  
  M missing_content2_missing_content2-untracked
  content2
  
  M missing_content2_missing_content4-tracked
  <<<<<<< working copy: 0447570f1af6 - test: local
  content4
  ||||||| base
  =======
  content2
  >>>>>>> merge rev:    85100b8c675b - test: remote
  
  M missing_content2_missing_content4-untracked
  <<<<<<< working copy: 0447570f1af6 - test: local
  content4
  ||||||| base
  =======
  content2
  >>>>>>> merge rev:    85100b8c675b - test: remote
  
  M missing_content2_missing_missing-tracked
  content2
  
  M missing_content2_missing_missing-untracked
  content2
  
  C missing_missing_content3_content3-tracked
  content3
  
  R missing_missing_content3_content3-untracked
  content3
  
  M missing_missing_content3_content4-tracked
  content4
  
  R missing_missing_content3_content4-untracked
  content4
  
  R missing_missing_content3_missing-tracked
  <missing>
  
  R missing_missing_content3_missing-untracked
  <missing>
  
  A missing_missing_missing_content4-tracked
  content4
  
  ? missing_missing_missing_content4-untracked
  content4
  
  missing_missing_missing_missing-tracked: $ENOENT$
  <missing>
  
  missing_missing_missing_missing-untracked: * (glob)
  <missing>

  $ for f in `$PYTHON $TESTDIR/generateworkingcopystates.py filelist 3`
  > do
  >   if test -f ${f}.orig
  >   then
  >     echo ${f}.orig:
  >     cat ${f}.orig
  >   fi
  > done
  content1_content2_content1_content4-tracked.orig:
  content4
  content1_content2_content2_content4-tracked.orig:
  content4
  content1_content2_content3_content3-tracked.orig:
  content3
  content1_content2_content3_content4-tracked.orig:
  content4
  content1_content2_missing_content4-tracked.orig:
  content4
  missing_content2_content2_content4-tracked.orig:
  content4
  missing_content2_content3_content3-tracked.orig:
  content3
  missing_content2_content3_content4-tracked.orig:
  content4
  missing_content2_missing_content4-tracked.orig:
  content4
  missing_content2_missing_content4-untracked.orig:
  content4

Re-resolve and check status

  $ hg resolve --unmark --all
  $ hg resolve --all --tool :local
  (no more unresolved files)
  $ hg resolve --unmark --all
  $ hg resolve --all --tool internal:merge3
  other [merge rev] changed content1_content2_content1_content1-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content1_content2-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  merging content1_content2_content1_content4-tracked
  other [merge rev] changed content1_content2_content1_content4-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content1_missing-tracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content1_missing-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  merging content1_content2_content2_content1-tracked
  other [merge rev] changed content1_content2_content2_content1-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content2_content2-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  merging content1_content2_content2_content4-tracked
  other [merge rev] changed content1_content2_content2_content4-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content2_missing-tracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content2_missing-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  merging content1_content2_content3_content1-tracked
  other [merge rev] changed content1_content2_content3_content1-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content3_content2-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  merging content1_content2_content3_content3-tracked
  other [merge rev] changed content1_content2_content3_content3-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  merging content1_content2_content3_content4-tracked
  other [merge rev] changed content1_content2_content3_content4-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content3_missing-tracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_content3_missing-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  merging content1_content2_missing_content1-tracked
  other [merge rev] changed content1_content2_missing_content1-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_missing_content2-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  merging content1_content2_missing_content4-tracked
  other [merge rev] changed content1_content2_missing_content4-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_missing_missing-tracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  other [merge rev] changed content1_content2_missing_missing-untracked which local [working copy] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  local [working copy] changed content1_missing_content1_content4-tracked which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  local [working copy] changed content1_missing_content3_content3-tracked which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  local [working copy] changed content1_missing_content3_content4-tracked which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  local [working copy] changed content1_missing_missing_content4-tracked which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  merging missing_content2_content2_content4-tracked
  merging missing_content2_content3_content3-tracked
  merging missing_content2_content3_content4-tracked
  merging missing_content2_missing_content4-tracked
  merging missing_content2_missing_content4-untracked
  warning: 1 conflicts while merging content1_content2_content1_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging content1_content2_content2_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging content1_content2_content3_content3-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging content1_content2_content3_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging content1_content2_missing_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging missing_content2_content2_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging missing_content2_content3_content3-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging missing_content2_content3_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging missing_content2_missing_content4-tracked! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging missing_content2_missing_content4-untracked! (edit, then use 'hg resolve --mark')
  [1]
  $ checkstatus > $TESTTMP/status2 2>&1
  $ cmp $TESTTMP/status1 $TESTTMP/status2 || diff -U8 $TESTTMP/status1 $TESTTMP/status2

Set up working directory again

  $ hg -q goto --clean 'desc(local)'
  $ hg purge
  $ $PYTHON $TESTDIR/generateworkingcopystates.py state 3 wc
  $ hg addremove -q --similarity 0
  $ hg forget *_*_*_*-untracked
  $ rm *_*_*_missing-*

Merge with checkunknown = warn, see that behavior is the same as before
  $ hg merge -f --tool internal:merge3 'desc("remote")' --config merge.checkunknown=warn > $TESTTMP/merge-output-2 2>&1
  [1]
  $ cmp $TESTTMP/merge-output-1 $TESTTMP/merge-output-2 || diff -U8 $TESTTMP/merge-output-1 $TESTTMP/merge-output-2
