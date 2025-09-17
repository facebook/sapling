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
  abort: EdenError: sapling::SaplingFetchError: Network Error: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
  
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
  abort: EdenError: a previous checkout was interrupted - please run `hg go 62236523d20eb09473170bc922c224800a9ec819` to resume it.
  If there are conflicts, run `hg go --clean 62236523d20eb09473170bc922c224800a9ec819` to discard changes, or `hg go --merge 62236523d20eb09473170bc922c224800a9ec819` to merge.
  [255]

Try without --clean:
  $ hg go 62236523d20eb09473170bc922c224800a9ec819
  abort: 1 conflicting file changes:
   dir/file
  (commit, shelve, goto --clean to discard all your changes, or goto --merge to merge them)
  [255]

Complete the checkout:
  $ hg update -qC 62236523d20eb09473170bc922c224800a9ec819

  $ hg st

File has the correct contents:
  $ cat dir/file
  changed
