#require eden no-windows

#testcases filtered unfiltered

#if filtered
  $ enable edensparse
  $ setconfig clone.eden-sparse-filter=
  $ setconfig remotefilelog.cachepath=$TESTTMP/filtered-cache
#else
  $ setconfig remotefilelog.cachepath=$TESTTMP/unfiltered-cache
#endif

  $ eden restart >/dev/null 2>&1

  $ newserver server
  $ drawdag <<EOS
  > C # C/dir/file = changed again\n
  > |
  > B # B/dir/file = changed\n
  > |
  > A # A/dir/file = foo
  >   # A/profile1 = dir/file\n
  >   # A/profile2 = dir/file\nsomething/else\n
  >   # bookmark master = A
  > EOS

  $ cd

#if filtered
  $ setconfig clone.eden-sparse-filter=profile1
#endif

  $ newclientrepo client server
Move between two commits to make sure we have root trees fetched. Errors fetching root trees make things fail hard.
  $ hg go -q $B
  $ hg go -q $C
  $ hg go -q $A
  $ echo mismatch > dir/file

  $ cat >> $HOME/.edenrc <<EOS
  > [experimental]
  > propagate-checkout-errors = true
  > EOS

Restart eden with error injected into eagerepo tree fetching.
  $ eden stop >/dev/null 2>&1
  $ FAILPOINTS=eagerepo::api::trees=return eden start >/dev/null 2>&1

  $ hg st
  M dir/file

Checkout fails since it can't fetch the tree:
  $ hg go -C $B
  abort: EdenError: sapling::SaplingBackingStoreError: Network Error: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
  
  Caused by:
      0: Network Error: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
      1: Network Error: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
      2: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
  [255]

Restart eden without injected network error.
  $ eden stop >/dev/null 2>&1
  $ eden start >/dev/null 2>&1

We are still in interrupted checkout state:
  $ hg st
  abort: EdenError: a previous checkout was interrupted - please run `sl go *` to resume it. (glob)
  If there are conflicts, run `sl go --clean *` to discard changes, or `sl go --merge *` to merge. (glob)
  [255]

Try without --clean:
  $ hg go $B
  abort: 1 conflicting file changes:
   dir/file
  (commit, shelve, goto --clean to discard all your changes, or goto --merge to merge them)
  [255]

Complete the checkout:
  $ hg update -qC $B

  $ hg st

File has the correct contents:
  $ cat dir/file
  changed

#if filtered
Set up another interrupted checkout to test filter ID mismatch:

  $ echo mismatched > dir/file

  $ eden stop >/dev/null 2>&1
  $ FAILPOINTS=eagerepo::api::trees=return eden start >/dev/null 2>&1
  $ hg go -qC $C
  abort: EdenError: sapling::SaplingBackingStoreError: Network Error: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
  
  Caused by:
      0: Network Error: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
      1: Network Error: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
      2: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
  [255]
  $ eden restart >/dev/null 2>&1

Change the filterdfs config while in interrupted checkout state.
  $ setconfig clone.eden-sparse-filter=profile2

Can resume checkout even though filter ids mismatch.
  $ hg go -qC $C

  $ hg st

  $ cat dir/file
  changed again
#endif
