#debugruntest-compatible

#require no-eden


  $ newrepo
  $ drawdag << 'EOS'
  > C   # C/A = (removed)
  > |
  > B
  > |
  > A
  > EOS

  $ hg up -q $C

--amend without --mark is unsupported (for now, alternative: regular cp + amend):

  $ hg cp --amend B C
  abort: --amend without --mark is not supported
  [255]

Mark 'C' as copied from 'B':

  $ hg cp B C
  C: not overwriting - file already committed
  (use 'hg copy --amend --mark' to amend the current commit)

  $ hg cp --amend --mark B C
  abort: 'B' and 'C' does not look similar
  (use --force to skip similarity check)
  [255]

  $ hg cp --amend --mark B C --force

Check result:

  $ hg status
  $ hg status --change . -AC C
  A C
    B

Change "C" to be renamed from "A":

  $ hg mv --amend --mark --mark A C
  abort: target path 'C' is already marked as copied from 'B'
  (use --force to skip this check)
  [255]

  $ hg mv --amend --mark A C --force

Check result:

  $ hg status
  $ hg status --change . -AC C
  A C
    A

Test behavior in middle of stack:
  $ newrepo
  $ drawdag <<EOS
  > C  # C/bar = bar
  > |
  > |
  > B  # B/bar = foo
  > |  # B/foo = (removed)
  > |
  > A  # A/foo = foo
  >    # drawdag.defaultfiles=false
  > EOS

  $ hg go -q $B
  $ tglog
  o  0dfdb4eecd4e 'C'
  │
  @  f9f49b656be4 'B'
  │
  o  84d740d4dbe5 'A'

FIXME: old B not obsoleted
  $ hg mv --mark --amend foo bar
  $ tglog
  @  eae398dea4ce 'B'
  │
  │ o  0dfdb4eecd4e 'C'
  │ │
  │ o  f9f49b656be4 'B'
  ├─╯
  o  84d740d4dbe5 'A'
