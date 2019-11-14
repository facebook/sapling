  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > crdump=
  > remotenames=
  > [crdump]
  > commitcloud=True
  > EOF

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
  $ hg debugcrdump -r . --config paths.default=xxxxx | grep commit_cloud
              "commit_cloud": false,

debugcrdump should upload the commit and commit_cloud should be true when
commitcloud is working
  $ hg debugcrdump -r . | grep commit_cloud
  remote: pushing 1 commit:
  remote:     9092f1db7931  added a
              "commit_cloud": true,

debugcrdump should not attempt to access the network if the commit was
previously backed up (as shown by the lack of error when given a faulty path)
  $ hg debugcrdump -r . --config ui.ssh=false | grep commit_cloud
              "commit_cloud": true,
