Tests of 'hg status --rev <rev>' to make sure status between <rev> and '.' get
combined correctly with the dirstate status.

  $ hg init

First commit

  $ python $TESTDIR/generate-working-copy-states.py state 2 1
  $ hg addremove --similarity 0
  adding content1_content1_content1-tracked
  adding content1_content1_content1-untracked
  adding content1_content1_content3-tracked
  adding content1_content1_content3-untracked
  adding content1_content1_missing-tracked
  adding content1_content1_missing-untracked
  adding content1_content2_content1-tracked
  adding content1_content2_content1-untracked
  adding content1_content2_content2-tracked
  adding content1_content2_content2-untracked
  adding content1_content2_content3-tracked
  adding content1_content2_content3-untracked
  adding content1_content2_missing-tracked
  adding content1_content2_missing-untracked
  adding content1_missing_content1-tracked
  adding content1_missing_content1-untracked
  adding content1_missing_content3-tracked
  adding content1_missing_content3-untracked
  adding content1_missing_missing-tracked
  adding content1_missing_missing-untracked
  $ hg commit -m first

Second commit

  $ python $TESTDIR/generate-working-copy-states.py state 2 2
  $ hg addremove --similarity 0
  removing content1_missing_content1-tracked
  removing content1_missing_content1-untracked
  removing content1_missing_content3-tracked
  removing content1_missing_content3-untracked
  removing content1_missing_missing-tracked
  removing content1_missing_missing-untracked
  adding missing_content2_content2-tracked
  adding missing_content2_content2-untracked
  adding missing_content2_content3-tracked
  adding missing_content2_content3-untracked
  adding missing_content2_missing-tracked
  adding missing_content2_missing-untracked
  $ hg commit -m second

Working copy

  $ python $TESTDIR/generate-working-copy-states.py state 2 wc
  $ hg addremove --similarity 0
  adding content1_missing_content1-tracked
  adding content1_missing_content1-untracked
  adding content1_missing_content3-tracked
  adding content1_missing_content3-untracked
  adding content1_missing_missing-tracked
  adding content1_missing_missing-untracked
  adding missing_missing_content3-tracked
  adding missing_missing_content3-untracked
  adding missing_missing_missing-tracked
  adding missing_missing_missing-untracked
  $ hg forget *_*_*-untracked
  $ rm *_*_missing-*

Status compared to parent of the working copy, i.e. the dirstate status

  $ hg status -A --rev 1 'glob:missing_content2_content3-tracked'
  M missing_content2_content3-tracked
  $ hg status -A --rev 1 'glob:missing_content2_content2-tracked'
  C missing_content2_content2-tracked
  $ hg status -A --rev 1 'glob:missing_missing_content3-tracked'
  A missing_missing_content3-tracked
  $ hg status -A --rev 1 'glob:missing_missing_content3-untracked'
  ? missing_missing_content3-untracked
  $ hg status -A --rev 1 'glob:missing_content2_*-untracked'
  R missing_content2_content2-untracked
  R missing_content2_content3-untracked
  R missing_content2_missing-untracked
  $ hg status -A --rev 1 'glob:missing_*_missing-tracked'
  ! missing_content2_missing-tracked
  ! missing_missing_missing-tracked
#if windows
  $ hg status -A --rev 1 'glob:missing_missing_missing-untracked'
  missing_missing_missing-untracked: The system cannot find the file specified
#else
  $ hg status -A --rev 1 'glob:missing_missing_missing-untracked'
  missing_missing_missing-untracked: No such file or directory
#endif

Status between first and second commit. Should ignore dirstate status.

  $ hg status -A --rev 0:1 'glob:content1_content2_*'
  M content1_content2_content1-tracked
  M content1_content2_content1-untracked
  M content1_content2_content2-tracked
  M content1_content2_content2-untracked
  M content1_content2_content3-tracked
  M content1_content2_content3-untracked
  M content1_content2_missing-tracked
  M content1_content2_missing-untracked
  $ hg status -A --rev 0:1 'glob:content1_content1_*'
  C content1_content1_content1-tracked
  C content1_content1_content1-untracked
  C content1_content1_content3-tracked
  C content1_content1_content3-untracked
  C content1_content1_missing-tracked
  C content1_content1_missing-untracked
  $ hg status -A --rev 0:1 'glob:missing_content2_*'
  A missing_content2_content2-tracked
  A missing_content2_content2-untracked
  A missing_content2_content3-tracked
  A missing_content2_content3-untracked
  A missing_content2_missing-tracked
  A missing_content2_missing-untracked
  $ hg status -A --rev 0:1 'glob:content1_missing_*'
  R content1_missing_content1-tracked
  R content1_missing_content1-untracked
  R content1_missing_content3-tracked
  R content1_missing_content3-untracked
  R content1_missing_missing-tracked
  R content1_missing_missing-untracked
  $ hg status -A --rev 0:1 'glob:missing_missing_*'

Status compared to one revision back, checking that the dirstate status
is correctly combined with the inter-revision status

  $ hg status -A --rev 0 'glob:content1_*_content[23]-tracked'
  M content1_content1_content3-tracked
  M content1_content2_content2-tracked
  M content1_content2_content3-tracked
  M content1_missing_content3-tracked
  $ hg status -A --rev 0 'glob:content1_*_content1-tracked'
  C content1_content1_content1-tracked
  C content1_content2_content1-tracked
  C content1_missing_content1-tracked
  $ hg status -A --rev 0 'glob:missing_*_content?-tracked'
  A missing_content2_content2-tracked
  A missing_content2_content3-tracked
  A missing_missing_content3-tracked
BROKEN: missing_content2_content[23]-untracked exist, so should be listed
  $ hg status -A --rev 0 'glob:missing_*_content?-untracked'
  ? missing_missing_content3-untracked
  $ hg status -A --rev 0 'glob:content1_*_*-untracked'
  R content1_content1_content1-untracked
  R content1_content1_content3-untracked
  R content1_content1_missing-untracked
  R content1_content2_content1-untracked
  R content1_content2_content2-untracked
  R content1_content2_content3-untracked
  R content1_content2_missing-untracked
  R content1_missing_content1-untracked
  R content1_missing_content3-untracked
  R content1_missing_missing-untracked
  $ hg status -A --rev 0 'glob:*_*_missing-tracked'
  ! content1_content1_missing-tracked
  ! content1_content2_missing-tracked
  ! content1_missing_missing-tracked
  ! missing_content2_missing-tracked
  ! missing_missing_missing-tracked
  $ hg status -A --rev 0 'glob:missing_*_missing-untracked'
