Tests of 'hg status --rev <rev>' to make sure status between <rev> and '.' get
combined correctly with the dirstate status.

  $ hg init
  $ touch .hgignore
  $ hg add .hgignore
  $ hg commit -m initial

First commit

  $ python $TESTDIR/generate-working-copy-states.py base
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

  $ python $TESTDIR/generate-working-copy-states.py parent
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

  $ python $TESTDIR/generate-working-copy-states.py wc
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

Status compared to one revision back

  $ hg status -A --rev 1 content1_content1_content1-tracked
  C content1_content1_content1-tracked
BROKEN: file appears twice; should be '!'
  $ hg status -A --rev 1 content1_content1_missing-tracked
  ! content1_content1_missing-tracked
  C content1_content1_missing-tracked
  $ hg status -A --rev 1 content1_content1_content1-untracked
  R content1_content1_content1-untracked
  $ hg status -A --rev 1 content1_content1_content3-tracked
  M content1_content1_content3-tracked
  $ hg status -A --rev 1 content1_content1_missing-untracked
  R content1_content1_missing-untracked
  $ hg status -A --rev 1 content1_content2_content2-tracked
  M content1_content2_content2-tracked
BROKEN: file appears twice; should be '!'
  $ hg status -A --rev 1 content1_content2_missing-tracked
  ! content1_content2_missing-tracked
  C content1_content2_missing-tracked
  $ hg status -A --rev 1 content1_content2_content2-untracked
  R content1_content2_content2-untracked
  $ hg status -A --rev 1 content1_content2_content3-tracked
  M content1_content2_content3-tracked
  $ hg status -A --rev 1 content1_content2_missing-untracked
  R content1_content2_missing-untracked
  $ hg status -A --rev 1 content1_missing_content3-tracked
  M content1_missing_content3-tracked
BROKEN: file appears twice; should be '!'
  $ hg status -A --rev 1 content1_missing_missing-tracked
  R content1_missing_missing-tracked
  ! content1_missing_missing-tracked
  $ hg status -A --rev 1 content1_missing_content3-untracked
  R content1_missing_content3-untracked
  $ hg status -A --rev 1 missing_content2_missing-tracked
  ! missing_content2_missing-tracked
BROKEN: file exists, so should be listed (as '?')
  $ hg status -A --rev 1 missing_content2_content2-untracked
  $ hg status -A --rev 1 missing_content2_content3-tracked
  A missing_content2_content3-tracked
  $ hg status -A --rev 1 missing_content2_missing-untracked
  $ hg status -A --rev 1 missing_missing_content3-untracked
  ? missing_missing_content3-untracked
