#require no-windows

  $ setconfig copytrace.fallback-to-content-similarity=True

test fold skipping content similarity check for large files
  $ newclientrepo
  $ drawdag <<'EOS'
  > B  # B/b = 1b\n2\n3\n
  > |
  > A  # A/a = 1\n2\n3\n
  > EOS

  $ hg go -q $B
  $ cp a c
  $ hg add c
  $ hg rm a
  $ hg ci -m 'add c, remove a'
  $ echo 1 >> d
  $ hg ci -Aqm 'add d'
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  ca0dda5636a7 add d
  │
  o  036a2c1ceb15 add c, remove a
  │
  o  c41733be38d4 B
  │
  o  d177b9890a58 A
  $ SL_LOG=copytrace=debug hg fold --from .^ --config copytrace.large-file-threshold=1 2>&1 | grep "file too large"
  DEBUG copytrace::rename_finders: file too large, skipping content similarity check large_file_threshold=ByteCount(1) source_content_len=6
  DEBUG copytrace::rename_finders: file too large, skipping content similarity check large_file_threshold=ByteCount(1) source_content_len=2
  $ hg st --change . --copies
  A c
  A d
  R a

test fold with content similarity check
  $ newclientrepo
  $ drawdag <<'EOS'
  > B  # B/b = 1b\n2\n3\n
  > |
  > A  # A/a = 1\n2\n3\n
  > EOS

  $ hg go -q $B
  $ cp a c
  $ hg add c
  $ hg rm a
  $ hg ci -m 'add c, remove a'
  $ echo 1 >> d
  $ hg ci -Aqm 'add d'
  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  ca0dda5636a7 add d
  │
  o  036a2c1ceb15 add c, remove a
  │
  o  c41733be38d4 B
  │
  o  d177b9890a58 A
  $ SL_LOG=copytrace=debug hg fold --from .^ --config copytrace.large-file-threshold=1MB 2>&1 | grep "file too large"
  [1]
  $ hg st --change . --copies
  A c
    a
  A d
  R a
