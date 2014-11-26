Set up a base, local, and remote changeset, as well as the working copy state.
Files names are of the form base_remote_local_working-copy. For example,
content1_content2_content1_content2-untracked represents a
file that was modified in the remote changeset, left untouched in the
local changeset, and then modified in the working copy to match the
remote content, then finally forgotten.

  $ hg init

Create base changeset

  $ python $TESTDIR/generate-working-copy-states.py state 3 1
  $ hg addremove -q --similarity 0
  $ hg commit -qm 'base'

Create remote changeset

  $ python $TESTDIR/generate-working-copy-states.py state 3 2
  $ hg addremove -q --similarity 0
  $ hg commit -qm 'remote'

Create local changeset

  $ hg update -q 0
  $ python $TESTDIR/generate-working-copy-states.py state 3 3
  $ hg addremove -q --similarity 0
  $ hg commit -qm 'local'

Set up working directory

  $ python $TESTDIR/generate-working-copy-states.py state 3 wc
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

  $ hg merge -f --tool internal:merge3 'desc("remote")'
  local changed content1_missing_content1_content4-tracked which remote deleted
  use (c)hanged version or (d)elete? c
  local changed content1_missing_content3_content3-tracked which remote deleted
  use (c)hanged version or (d)elete? c
  local changed content1_missing_content3_content4-tracked which remote deleted
  use (c)hanged version or (d)elete? c
  local changed content1_missing_missing_content4-tracked which remote deleted
  use (c)hanged version or (d)elete? c
  remote changed content1_content2_content1_content1-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content1_content2-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content1_content4-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content1_missing-tracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content1_missing-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content2_content1-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content2_content2-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content2_content4-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content2_missing-tracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content2_missing-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content3_content1-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content3_content2-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content3_content3-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content3_content4-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content3_missing-tracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_content3_missing-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_missing_content1-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_missing_content2-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_missing_content4-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_missing_missing-tracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote changed content1_content2_missing_missing-untracked which local deleted
  use (c)hanged version or leave (d)eleted? c
  merging content1_content2_content1_content4-tracked
  warning: conflicts during merge.
  merging content1_content2_content1_content4-tracked incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging content1_content2_content2_content1-tracked
  merging content1_content2_content2_content4-tracked
  warning: conflicts during merge.
  merging content1_content2_content2_content4-tracked incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging content1_content2_content3_content1-tracked
  merging content1_content2_content3_content3-tracked
  warning: conflicts during merge.
  merging content1_content2_content3_content3-tracked incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging content1_content2_content3_content4-tracked
  warning: conflicts during merge.
  merging content1_content2_content3_content4-tracked incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging content1_content2_missing_content1-tracked
  merging content1_content2_missing_content4-tracked
  warning: conflicts during merge.
  merging content1_content2_missing_content4-tracked incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging missing_content2_content2_content4-tracked
  warning: conflicts during merge.
  merging missing_content2_content2_content4-tracked incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging missing_content2_content3_content3-tracked
  warning: conflicts during merge.
  merging missing_content2_content3_content3-tracked incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging missing_content2_content3_content4-tracked
  warning: conflicts during merge.
  merging missing_content2_content3_content4-tracked incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging missing_content2_missing_content4-tracked
  warning: conflicts during merge.
  merging missing_content2_missing_content4-tracked incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging missing_content2_missing_content4-untracked
  warning: conflicts during merge.
  merging missing_content2_missing_content4-untracked incomplete! (edit conflicts, then use 'hg resolve --mark')
  39 files updated, 3 files merged, 8 files removed, 10 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

Check which files need to be resolved (should correspond to the ouput above).
This should be the files for which the base (1st filename segment), the remote
(2nd segment) and the working copy (4th segment) are all different.

Interestingly, one untracked file got merged and added, which corresponds the
odd 'if force and branchmerge and different' case in manifestmerge().

  $ hg resolve -l
  U content1_content2_content1_content4-tracked
  R content1_content2_content2_content1-tracked
  U content1_content2_content2_content4-tracked
  R content1_content2_content3_content1-tracked
  U content1_content2_content3_content3-tracked
  U content1_content2_content3_content4-tracked
  R content1_content2_missing_content1-tracked
  U content1_content2_missing_content4-tracked
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

*_missing_missing_missing-tracked is reported as removed ('R'), which
doesn't make sense since the file did not exist in the parent, but on the
other hand, merged-in additions are reported as modifications, which is
almost as strange.

missing_missing_content3_missing-tracked becomes removed ('R'), even though
the remote side did not touch the file

  $ for f in `python $TESTDIR/generate-working-copy-states.py filelist 3`
  > do
  >   echo
  >   hg status -A $f
  >   if test -f $f
  >   then
  >     cat $f
  >   else
  >     echo '<missing>'
  >   fi
  >   if test -f ${f}.orig
  >   then
  >     echo ${f}.orig:
  >     cat ${f}.orig
  >   fi
  > done
  
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
  <<<<<<< local: 0447570f1af6 - test: local
  content4
  ||||||| base
  content1
  =======
  content2
  >>>>>>> other: 85100b8c675b  - test: remote
  content1_content2_content1_content4-tracked.orig:
  content4
  
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
  <<<<<<< local: 0447570f1af6 - test: local
  content4
  ||||||| base
  content1
  =======
  content2
  >>>>>>> other: 85100b8c675b  - test: remote
  content1_content2_content2_content4-tracked.orig:
  content4
  
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
  <<<<<<< local: 0447570f1af6 - test: local
  content3
  ||||||| base
  content1
  =======
  content2
  >>>>>>> other: 85100b8c675b  - test: remote
  content1_content2_content3_content3-tracked.orig:
  content3
  
  M content1_content2_content3_content3-untracked
  content2
  
  M content1_content2_content3_content4-tracked
  <<<<<<< local: 0447570f1af6 - test: local
  content4
  ||||||| base
  content1
  =======
  content2
  >>>>>>> other: 85100b8c675b  - test: remote
  content1_content2_content3_content4-tracked.orig:
  content4
  
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
  <<<<<<< local: 0447570f1af6 - test: local
  content4
  ||||||| base
  content1
  =======
  content2
  >>>>>>> other: 85100b8c675b  - test: remote
  content1_content2_missing_content4-tracked.orig:
  content4
  
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
  
  R content1_missing_missing_content1-tracked
  <missing>
  
  ? content1_missing_missing_content1-untracked
  content1
  
  A content1_missing_missing_content4-tracked
  content4
  
  ? content1_missing_missing_content4-untracked
  content4
  
  R content1_missing_missing_missing-tracked
  <missing>
  
  content1_missing_missing_missing-untracked: * (glob)
  <missing>
  
  C missing_content2_content2_content2-tracked
  content2
  
  M missing_content2_content2_content2-untracked
  content2
  
  M missing_content2_content2_content4-tracked
  <<<<<<< local: 0447570f1af6 - test: local
  content4
  ||||||| base
  =======
  content2
  >>>>>>> other: 85100b8c675b  - test: remote
  missing_content2_content2_content4-tracked.orig:
  content4
  
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
  <<<<<<< local: 0447570f1af6 - test: local
  content3
  ||||||| base
  =======
  content2
  >>>>>>> other: 85100b8c675b  - test: remote
  missing_content2_content3_content3-tracked.orig:
  content3
  
  M missing_content2_content3_content3-untracked
  content2
  
  M missing_content2_content3_content4-tracked
  <<<<<<< local: 0447570f1af6 - test: local
  content4
  ||||||| base
  =======
  content2
  >>>>>>> other: 85100b8c675b  - test: remote
  missing_content2_content3_content4-tracked.orig:
  content4
  
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
  <<<<<<< local: 0447570f1af6 - test: local
  content4
  ||||||| base
  =======
  content2
  >>>>>>> other: 85100b8c675b  - test: remote
  missing_content2_missing_content4-tracked.orig:
  content4
  
  M missing_content2_missing_content4-untracked
  <<<<<<< local: 0447570f1af6 - test: local
  content4
  ||||||| base
  =======
  content2
  >>>>>>> other: 85100b8c675b  - test: remote
  missing_content2_missing_content4-untracked.orig:
  content4
  
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
  
  R missing_missing_missing_missing-tracked
  <missing>
  
  missing_missing_missing_missing-untracked: * (glob)
  <missing>
