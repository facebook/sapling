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

  $ SL_LOG=file_fetches=trace sl commit --no-move-detection -qm change 2> $TESTTMP/fetches
  $ sed '/"content"/d' $TESTTMP/fetches
  TRACE file_fetches: attrs=["history"] length=Some(1) keys=["first", "second"]

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

Merge commits need the histories of both parent file revisions in one batch.

  $ newserver merge-server
  $ drawdag <<'EOS'
  > B C  # B/first = left first\n
  > |/   # B/second = left second\n
  > A    # A/first = base first\n
  >      # A/second = base second\n
  >      # C/first = right first\n
  >      # C/second = right second\n
  > EOS
  $ sl bookmark -r $B left
  $ sl bookmark -r $C right
  $ newclientrepo merge-client merge-server left right
  $ sl go -q left
  $ sl merge -q right --tool internal:local
  $ newcachedir
  $ sl prefetch -r left -r right

  $ SL_LOG=file_fetches=trace sl commit -qm merge 2> $TESTTMP/fetches
  $ sed '/"content"/d' $TESTTMP/fetches
  TRACE file_fetches: attrs=["history"] length=None keys=["first", "first", "second", "second"]
  $ sl status
  $ sl log -r 'parents(.)' -T '{desc}\n' | sort
  B
  C
  $ sl cat -r . first second
  left first
  left second
