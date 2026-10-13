#require no-eden

  $ . "$TESTDIR/library.sh"
  $ eagerepo
  $ setconfig drawdag.defaultfiles=false
  $ newserver server
  $ drawdag <<'EOS'
  > A  # A/first = first\n
  >    # A/second = second\n
  >    # A/removed = removed\n
  >    # A/unchanged = unchanged\n
  > EOS
  $ sl bookmark -r $A main

  $ newclientrepo client server main
  $ echo "first changed" > first
  $ echo "second changed" > second
  $ echo added > added
  $ sl add added
  $ sl rm removed
  $ newcachedir
  $ sl prefetch -r .

Commit modified files with cached content and an empty history cache.
FIXME: Batch the parent file revisions instead of fetching history per file.

  $ SL_LOG=file_fetches=trace sl commit --no-move-detection -qm change 2> $TESTTMP/fetches
  $ sed '/"content"/d' $TESTTMP/fetches
  TRACE file_fetches: attrs=["history"] length=Some(1) keys=["first"]
  TRACE file_fetches: attrs=["history"] length=Some(1) keys=["second"]

  $ sl status
  $ sl log -r . -T '{files % "{file}\n"}'
  added
  first
  removed
  second
  $ sl cat -r . first second added unchanged
  added
  first changed
  second changed
  unchanged
