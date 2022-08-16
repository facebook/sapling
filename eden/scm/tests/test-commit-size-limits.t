#debugruntest-compatible
  $ enable commitextras
  $ newrepo
  $ echo data > file
  $ hg add file

Test commit message limit
  $ hg commit -m "long message" --config commit.description-size-limit=11
  abort: commit message length (12) exceeds configured limit (11)
  [255]
  $ hg commit -m "long message" --config commit.description-size-limit=12

  $ echo data >> file

Test extras limit (limit includes 13 bytes for "branch=default")
  $ hg commit -m "message" --extra "testextra=long value" \
  >   --config commit.extras-size-limit=31
  abort: commit extras total size (32) exceeds configured limit (31)
  [255]
  $ hg commit -m "message" --extra "testextra=long value" \
  >   --config commit.extras-size-limit=32
