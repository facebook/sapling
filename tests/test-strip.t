  $ . "$TESTDIR/library.sh"

Populate the db with an initial commit

  $ initclient client
  $ cd client
  $ echo x > x
  $ hg commit -qAm x
  $ echo y > y
  $ hg commit -qAm y
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo z > z
  $ hg commit -qAm z
  $ hg bookmark foo
  $ cd ..

Create two masters

  $ initserver master masterrepo
  $ initserver master2 masterrepo
  $ cd master
  $ hg log
  $ hg pull -q ../client

  $ cd ../master2
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  2 z foo
  |
  | o  1 y
  |/
  o  0 x
  

Stripping normally should fail

  $ hg strip -r 1
  saved backup bundle to $TESTTMP/master2/.hg/strip-backup/d34c38483be9-3839604f-backup.hg (glob)
  transaction abort!
  rollback completed
  strip failed, backup bundle stored in '$TESTTMP/master2/.hg/strip-backup/d34c38483be9-3839604f-backup.hg'
  strip failed, unrecovered changes stored in '$TESTTMP/master2/.hg/strip-backup/d34c38483be9-48128f20-temp.hg'
  (fix the problem, then recover the changesets with "hg unbundle '$TESTTMP/master2/.hg/strip-backup/d34c38483be9-48128f20-temp.hg'")
  abort: invalid repo change - only hg push and pull are allowed
  [255]

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 3 changesets, 3 total revisions

Strip using sqlstrip

  $ hg sqlstrip 1
  abort: You must pass --i-know-what-i-am-doing to run this command. If you have multiple servers using the database, this command will break your servers until you run it on each one. Only the Mercurial server admins should ever run this.
  [255]

  $ hg sqlstrip --i-know-what-i-am-doing 1
  *** YOU ARE ABOUT TO DELETE HISTORY (MANDATORY 5 SECOND WAIT) ***
  stripping locally
  saved backup bundle to $TESTTMP/master2/.hg/strip-backup/bc3a71defa4a-f38e411b-backup.hg (glob)
  stripping from the database
  deleting old references
  adding new head references
  adding new tip reference
  adding new bookmark references
  deleting revision data

Verify master2 is stripped

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  0 x foo
  
Verify master is broken

  $ cd ../master
  $ hg log 2>&1 | egrep 'hgext_hgsql.CorruptionException'
  hgext_hgsql.CorruptionException: heads don't match after sync

Run sqlstrip on master as well

  $ hg sqlstrip --i-know-what-i-am-doing 1
  *** YOU ARE ABOUT TO DELETE HISTORY (MANDATORY 5 SECOND WAIT) ***
  stripping locally
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/bc3a71defa4a-f38e411b-backup.hg (glob)
  stripping from the database
  deleting old references
  adding new head references
  adding new tip reference
  adding new bookmark references
  deleting revision data

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  0 x foo
  
Commit after the strip

  $ hg up -q 0
  $ echo z > z
  $ hg commit -qAm z

  $ cd ../master2
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  1 z
  |
  o  0 x foo
  
Attempt to strip a non-existant rev

  $ hg sqlstrip --i-know-what-i-am-doing 5
  *** YOU ARE ABOUT TO DELETE HISTORY (MANDATORY 5 SECOND WAIT) ***
  abort: revision 5 is not in the repo
  [255]

Attempt to strip a non-integer

  $ hg sqlstrip --i-know-what-i-am-doing master
  *** YOU ARE ABOUT TO DELETE HISTORY (MANDATORY 5 SECOND WAIT) ***
  abort: specified rev must be an integer: 'master'
  [255]
