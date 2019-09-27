  $ hg init

Set up history and working copy

  $ $PYTHON $TESTDIR/generateworkingcopystates.py state 2 1
  $ hg addremove -q --similarity 0
  $ hg commit -m first

  $ $PYTHON $TESTDIR/generateworkingcopystates.py state 2 2
  $ hg addremove -q --similarity 0
  $ hg commit -m second

  $ $PYTHON $TESTDIR/generateworkingcopystates.py state 2 wc
  $ hg addremove -q --similarity 0
  $ hg forget *_*_*-untracked
  $ rm *_*_missing-*

Test status

  $ hg st -A 'set:modified()'
  M content1_content1_content3-tracked
  M content1_content2_content1-tracked
  M content1_content2_content3-tracked
  M missing_content2_content3-tracked

  $ hg st -A 'set:added()'
  A content1_missing_content1-tracked
  A content1_missing_content3-tracked
  A missing_missing_content3-tracked

  $ hg st -A 'set:removed()'
  R content1_content1_content1-untracked
  R content1_content1_content3-untracked
  R content1_content1_missing-untracked
  R content1_content2_content1-untracked
  R content1_content2_content2-untracked
  R content1_content2_content3-untracked
  R content1_content2_missing-untracked
  R missing_content2_content2-untracked
  R missing_content2_content3-untracked
  R missing_content2_missing-untracked

  $ hg st -A 'set:deleted()'
  ! content1_content1_missing-tracked
  ! content1_content2_missing-tracked
  ! content1_missing_missing-tracked
  ! missing_content2_missing-tracked
  ! missing_missing_missing-tracked

  $ hg st -A 'set:missing()'
  ! content1_content1_missing-tracked
  ! content1_content2_missing-tracked
  ! content1_missing_missing-tracked
  ! missing_content2_missing-tracked
  ! missing_missing_missing-tracked

  $ hg st -A 'set:unknown()'
  ? content1_missing_content1-untracked
  ? content1_missing_content3-untracked
  ? missing_missing_content3-untracked

  $ hg st -A 'set:clean()'
  C content1_content1_content1-tracked
  C content1_content2_content2-tracked
  C missing_content2_content2-tracked

Test log

  $ hg log -T '{rev}\n' --stat 'set:modified()'
  1
   content1_content2_content1-tracked |  2 +-
   content1_content2_content3-tracked |  2 +-
   missing_content2_content3-tracked  |  1 +
   3 files changed, 3 insertions(+), 2 deletions(-)
  
  0
   content1_content1_content3-tracked |  1 +
   content1_content2_content1-tracked |  1 +
   content1_content2_content3-tracked |  1 +
   3 files changed, 3 insertions(+), 0 deletions(-)
  
Test revert

  $ hg revert 'set:modified()'
  reverting content1_content1_content3-tracked
  reverting content1_content2_content1-tracked
  reverting content1_content2_content3-tracked
  reverting missing_content2_content3-tracked

  $ hg revert 'set:added()'
  forgetting content1_missing_content1-tracked
  forgetting content1_missing_content3-tracked
  forgetting missing_missing_content3-tracked

  $ hg revert 'set:removed()'
  undeleting content1_content1_content1-untracked
  undeleting content1_content1_content3-untracked
  undeleting content1_content1_missing-untracked
  undeleting content1_content2_content1-untracked
  undeleting content1_content2_content2-untracked
  undeleting content1_content2_content3-untracked
  undeleting content1_content2_missing-untracked
  undeleting missing_content2_content2-untracked
  undeleting missing_content2_content3-untracked
  undeleting missing_content2_missing-untracked

  $ hg revert 'set:deleted()'
  reverting content1_content1_missing-tracked
  reverting content1_content2_missing-tracked
  forgetting content1_missing_missing-tracked
  reverting missing_content2_missing-tracked
  forgetting missing_missing_missing-tracked

  $ hg revert 'set:unknown()'

  $ hg revert 'set:clean()'
