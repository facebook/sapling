Tests of 'hg status --rev <rev>' to make sure status between <rev> and '.' get
combined correctly with the dirstate status.

  $ hg init
  $ touch .hgignore
  $ hg add .hgignore
  $ hg commit -m initial

First commit

  $ echo a >content1_content1_content1-tracked
  $ echo a >content1_content1_missing-tracked
  $ echo a >content1_content1_content1-untracked
  $ echo a >content1_content1_content3-tracked
  $ echo a >content1_content1_missing-untracked
  $ echo a >content1_content2_content2-tracked
  $ echo a >content1_content2_missing-tracked
  $ echo a >content1_content2_content2-untracked
  $ echo a >content1_content2_content3-tracked
  $ echo a >content1_content2_missing-untracked
  $ echo a >content1_missing_content3-tracked
  $ echo a >content1_missing_missing-tracked
  $ echo a >content1_missing_content3-untracked
  $ hg commit -Aqm first

Second commit

  $ echo b >missing_content2_missing-tracked
  $ echo b >missing_content2_content2-untracked
  $ echo b >missing_content2_content3-tracked
  $ echo b >missing_content2_missing-untracked
  $ echo b >content1_content2_content2-tracked
  $ echo b >content1_content2_content3-tracked
  $ echo b >content1_content2_content2-untracked
  $ echo b >content1_content2_content3-tracked
  $ echo b >content1_content2_missing-untracked
  $ hg rm content1_missing_content3-tracked
  $ hg rm content1_missing_missing-tracked
  $ hg rm content1_missing_content3-untracked
  $ hg commit -Aqm second

Working copy

  $ echo c >content1_content1_content3-tracked
  $ echo c >content1_content2_content3-tracked
  $ echo c >missing_content2_content3-tracked
  $ echo c >content1_missing_content3-tracked && hg add content1_missing_content3-tracked
  $ echo c >content1_missing_missing-tracked && hg add content1_missing_missing-tracked && rm content1_missing_missing-tracked
  $ echo c >content1_missing_content3-untracked
  $ hg rm content1_content2_missing-untracked
  $ hg rm content1_content1_missing-untracked
  $ hg rm missing_content2_missing-untracked
  $ rm content1_content1_missing-tracked
  $ rm content1_content2_missing-tracked
  $ rm missing_content2_missing-tracked
  $ hg forget content1_content1_content1-untracked
  $ hg forget content1_content2_content2-untracked
  $ hg forget missing_content2_content2-untracked
  $ touch missing_missing_content3-untracked

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
  $ hg status -A --rev 1 missing_content2_content2-untracked
  $ hg status -A --rev 1 missing_content2_content3-tracked
  A missing_content2_content3-tracked
  $ hg status -A --rev 1 missing_content2_missing-untracked
  $ hg status -A --rev 1 missing_missing_content3-untracked
  ? missing_missing_content3-untracked

Status compared to two revisions back

  $ hg status -A --rev 0 content1_content1_content1-tracked
  A content1_content1_content1-tracked
  $ hg status -A --rev 0 content1_content1_missing-tracked
  ! content1_content1_missing-tracked
BROKEN: file exists, so should be listed (as '?')
  $ hg status -A --rev 0 content1_content1_content1-untracked
  $ hg status -A --rev 0 content1_content1_content3-tracked
  A content1_content1_content3-tracked
  $ hg status -A --rev 0 content1_content1_missing-untracked
  $ hg status -A --rev 0 content1_content2_content2-tracked
  A content1_content2_content2-tracked
  $ hg status -A --rev 0 content1_content2_missing-tracked
  ! content1_content2_missing-tracked
BROKEN: file exists, so should be listed (as '?')
  $ hg status -A --rev 0 content1_content2_content2-untracked
  $ hg status -A --rev 0 content1_content2_content3-tracked
  A content1_content2_content3-tracked
  $ hg status -A --rev 0 content1_content2_missing-untracked
  $ hg status -A --rev 0 content1_missing_content3-tracked
  A content1_missing_content3-tracked
  $ hg status -A --rev 0 content1_missing_missing-tracked
  ! content1_missing_missing-tracked
  $ hg status -A --rev 0 content1_missing_content3-untracked
  ? content1_missing_content3-untracked
  $ hg status -A --rev 0 missing_content2_missing-tracked
  ! missing_content2_missing-tracked
BROKEN: file exists, so should be listed (as '?')
  $ hg status -A --rev 0 missing_content2_content2-untracked
  $ hg status -A --rev 0 missing_content2_content3-tracked
  A missing_content2_content3-tracked
  $ hg status -A --rev 0 missing_content2_missing-untracked
