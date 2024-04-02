#debugruntest-compatible

#require no-eden


  $ configure modernclient
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig clone.nativecheckout=True
  $ setconfig checkout.use-rust=true

Create a repo that touches a few files
  $ newclientrepo client1 test:server
  $ mkdir dir1 dir2
  $ touch dir1/x
  $ touch dir2/x
  $ hg commit -Aqm 'initial commit' 2>/dev/null
  $ hg push --to master --create -q
  $ cd ..

Bare clone the repo
  $ newclientrepo client2
  $ setconfig paths.default=test:server
  $ hg pull -q

Set a failpoint to force incomplete checkout.
  $ FAILPOINTS=checkout-post-progress=return hg checkout tip
  abort: checkout error: Error set by checkout-post-progress FAILPOINTS
  [255]

Verify we see the warning for other commands
  $ hg log -r .
  warning: this repository appears to have not finished cloning - run 'hg checkout --continue' to resume the clone
  commit:      000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  

Verify we cannot specify --continue and a rev
  $ hg checkout master --continue
  abort: can't specify a destination commit and --continue
  [255]

Verify the checkout resumes where it left off
  $ SL_LOG=checkout=debug hg checkout --continue 2>&1 | grep skipped_count
  DEBUG checkout:apply_store: checkout: skipped files based on progress skipped_count=2

Verify we can disable resumable checkouts
  $ hg checkout -q null
  $ mkdir dir2
  $ chmod -R a-w dir2
  $ hg checkout tip --config checkout.resumable=False
  abort: * (glob)
  [255]
  $ chmod -R a+w dir2
  $ test -f .hg/updateprogress
  [1]
  $ chmod -R a-w dir2
