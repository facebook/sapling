#debugruntest-incompatible
#require eden no-windows

  $ newserver server
  $ drawdag << 'EOS'
  > B  # B/dir/.slacl = acl config
  > |   # B/dir/file.txt = target
  > |
  > A  # A/dir/file.txt = base
  > EOS

  $ newclientrepo client server

Restart Eden with tree fetching broken.

  $ FAILPOINTS=eagerepo::api::trees=return eden restart >/dev/null 2>&1

Checkout fails and leaves Eden in an interrupted checkout state.

  $ sl go -C $B
  abort: EdenError: sapling::SaplingBackingStoreError: Network Error: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
  
  Caused by:
      0: Network Error: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
      1: Network Error: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
      2: server responded 500 Internal Server Error for eager://$TESTTMP/server/trees: failpoint. Headers: {}
  [255]

The checkout remains interrupted and asks the user to resume it.

  $ sl st
  abort: EdenError: a previous checkout was interrupted - please run `sl go *` to resume it. (glob)
  If there are conflicts, run `sl go --clean *` to discard changes, or `sl go --merge *` to merge. (glob)
  [255]

The suggested resume command succeeds without walking into the restricted
directory.

  $ sl go $B
  $ sl st
