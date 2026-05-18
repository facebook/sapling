
#require no-eden


  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig clone.nativecheckout=True
  $ setconfig checkout.use-rust=true

Create a repo that touches a few files
  $ newclientrepo client1 server
  $ mkdir dir1 dir2
  $ touch dir1/x
  $ touch dir2/x
  $ sl commit -Aqm 'initial commit' 2>/dev/null
  $ sl push --to master --create -q
  $ cd ..

Bare clone the repo
  $ newclientrepo client2
  $ setconfig paths.default=test:server
  $ sl pull -q

Set a failpoint to force incomplete checkout.
  $ FAILPOINTS=checkout-post-progress=return sl checkout tip
  abort: checkout errors:
   Error set by checkout-post-progress FAILPOINTS
  [255]

Verify we see the warning for other commands
  $ sl log -r .
  warning: this repository appears to have not finished cloning - run 'sl checkout --continue' to resume the clone
  commit:      000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  

Verify we cannot specify --continue and a rev
  $ sl checkout master --continue
  abort: can't specify a destination commit and --continue
  [255]

Verify the checkout resumes where it left off
  $ SL_LOG=checkout=debug sl checkout --continue 2>&1 | grep skipped_count
  DEBUG checkout:apply_store: checkout: skipped files based on progress skipped_count=2

Verify we can disable resumable checkouts
  $ sl checkout -q null
  $ mkdir dir2
  $ chmod -R a-w dir2
  $ sl checkout tip --config checkout.resumable=False
  abort: * (glob)
   dir2/x: Permission denied (os error 13) (unix-permissions !)
  [255]
  $ chmod -R a+w dir2
  $ test -f .sl/updateprogress
  [1]
  $ chmod -R a-w dir2
