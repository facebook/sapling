#debugruntest-compatible
  $ eagerepo
  $ newrepo

  $ echo 1 > foo
  $ echo 2 > bar
  $ hg add . -q

Commit should fail if the number of changed files exceeds the limit
  $ hg commit -m init --config commit.file-count-limit=1
  abort: commit file count (2) exceeds configured limit (1)
  (use '--config commit.file-count-limit=N' cautiously to override)
  [255]

Commit should succeed if the number of changed files <= the limit
  $ hg commit -m init --config commit.file-count-limit=2
  $ hg log -G -T '{desc}'
  @  init
