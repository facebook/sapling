#chg-compatible

  $ configure modern
  $ setconfig clone.nativecheckout=True
  $ newserver server

Create a repo that touches a few files
  $ newremoterepo client1
  $ mkdir dir1 dir2
  $ for i in `seq 0 5000` ; do touch dir1/$i ; done
  $ touch dir2/x
  $ hg commit -Aqm 'initial commit' 2>/dev/null
  $ hg push --to master --create -q
  $ cd ..

Bare clone the repo
  $ newremoterepo client2
  $ hg pull -q

#if no-windows
Make some of the directories we are about to checkout read-only so we encounter
an error mid-checkout.
  $ mkdir dir2
  $ chmod -R a-w dir2
  $ hg checkout tip --config remotefilelog.debug=False
  abort: Permission denied* (glob)
  [255]

Verify we see the warning for other commands
  $ hg log -r .
  warning: this repository appears to have not finished cloning - run 'hg checkout --continue' to resume the clone
  commit:      000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  

Verify we cannot specify --continue and a rev
  $ hg checkout master --continue
  warning: this repository appears to have not finished cloning - run 'hg checkout --continue' to resume the clone
  abort: cannot specify --continue and a update revision
  [255]

Verify the checkout resumes where it left off
- [^0].* tells it to not match "0", so we can ensure some files were skipped.
  $ chmod -R a+w dir2
  $ EDENSCM_LOG=checkout=debug hg checkout --continue
  warning: this repository appears to have not finished cloning - run 'hg checkout --continue' to resume the clone
  continuing checkout to '*' (glob)
  DEBUG checkout::prefetch: skip prefetch for non-lazychangelog
   INFO from_diff: checkout::actions: enter
   INFO from_diff: checkout::actions: exit
  DEBUG checkout: Skipping checking out [^0].* files since they're already written (re)
  5002 files updated, 0 files merged, 0 files removed, 0 files unresolved

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
#endif
