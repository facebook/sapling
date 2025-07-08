#modern-config-incompatible

#require no-eden


  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

  $ enable crdump
  $ setconfig crdump.commitcloud=true

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ../

  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ echo a >> a
  $ hg commit -Aqm "added a" --config infinitepushbackup.autobackup=False

commit_cloud should be false when commitcloud is broken
  $ setconfig treemanifest.http=0
  $ hg debugcrdump -r . --config experimental.upload-mutations=invalid | grep commit_cloud
              "commit_cloud": false,

don't report commit_cloud=true when the upload didn't work
  $ FAILPOINTS="eagerepo::api::uploadchangesets=return(empty)" hg debugcrdump -r . 2>/dev/null | grep commit_cloud
              "commit_cloud": false,

fail early if commitcloudrequired is set and some commits fail to upload to commit cloud
  $ FAILPOINTS="eagerepo::api::uploadchangesets=return(empty)" hg debugcrdump -r . --config crdump.commitcloudrequired=True
  abort: failed to upload commits to commit cloud: 9092f1db7931
  [255]

debugcrdump should upload the commit and commit_cloud should be true when
commitcloud is working
  $ hg debugcrdump -r . 2>/dev/null | grep commit_cloud
              "commit_cloud": true,

debugcrdump should not attempt to access the network if the commit was
previously backed up (as shown by the lack of error when given a faulty path)
  $ hg debugcrdump -r . --config ui.ssh=true | grep commit_cloud
              "commit_cloud": true,
