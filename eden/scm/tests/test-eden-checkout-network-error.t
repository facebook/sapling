#require eden no-windows

  $ newserver server
  $ drawdag <<EOS
  > B # B/dir/file = changed\n
  > |
  > A # A/dir/file = foo
  >   # bookmark master = A
  > EOS

  $ newclientrepo client server
Move between two commits to make sure we have root trees fetched. Errors fetching root trees make things fail hard.
  $ hg go -q $B
  $ hg go -q $A
  $ echo mismatch > dir/file

Restart eden with error injected into eagerepo tree fetching.
  $ eden stop >/dev/null 2>&1
  $ FAILPOINTS=eagerepo::api::trees=return eden start >/dev/null 2>&1

  $ hg st
  M dir/file

FIXME: checkout should fail
  $ hg go -C $B 
  update complete

FIXME: status is clean but we have the wrong file content
  $ hg st
  $ cat dir/file
  mismatch
