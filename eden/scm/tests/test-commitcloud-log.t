#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ enable infinitepush commitcloud
  $ configure dummyssh
  $ setconfig infinitepush.branchpattern="re:scratch/.*"
  $ setconfig commitcloud.hostname=testhost
  $ setconfig experimental.graphstyle.grandparent=2.
  $ setconfig templatealias.sl_cloud="\"{truncatelonglines(node, 6)} {ifeq(phase, 'public', '(public)', '')} {ifeq(phase, 'draft', author, '')} {date|isodate} {bookmarks}\\n{desc|firstline}\\n \""

  $ setconfig remotefilelog.reponame=server

  $ hg init server
  $ cd server
  $ setconfig infinitepush.server=yes infinitepush.indextype=disk infinitepush.storetype=disk infinitepush.reponame=testrepo

Make the clone of the server
  $ hg clone ssh://user@dummy/server client -q
  $ cd client
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation="$TESTTMP"

Tests for hg cloud log
  $ cat > $TESTTMP/usersmartlogdata << EOF
  > {
  >   "smartlog": {
  >     "nodes": []
  >   }
  > }
  > EOF
  $ hg cloud log
  the repository is not connected to any workspace, assuming the 'default' workspace

  $ cat > $TESTTMP/usersmartlogdata << EOF
  > {
  >   "smartlog": {
  >     "nodes": [
  >       { "node": "0067e44d36d919bec1bff6ac65d277e8e0dc2250", "phase": "draft", "author": "Test User", "date": 1527626185, "message": "some commit", "parents": ["4b1141993451c32f5e1c285ddc88468255cdccf2"], "bookmarks": [] },
  >       { "node": "30443c40415321c0157d3798f14c51068edb428d", "phase": "public", "author": "Test User", "date": 1529511690, "message": "some commit", "parents": ["5526fe82a2b98fb5f3a340f21712a3437ddeb300"], "bookmarks": [] },
  >       { "node": "4b1141993451c32f5e1c285ddc88468255cdccf2", "phase": "public", "author": "Test User", "date": 1527625388, "message": "some commit", "parents": ["42a2e3678e5e79c482a5eb4af808429fc044ae88"], "bookmarks": [] },
  >       { "node": "685a62272258b3bd4d71ac0b331486276b3c2599", "phase": "draft", "author": "Test User", "date": 1531408804, "message": "some commit", "parents": ["aa84f0443f949a6accca6d67b2790d2f37927451"], "bookmarks": [] },
  >       { "node": "717dccd1a732f794c51df27f7ba143c5c743d770", "phase": "draft", "author": "Test User", "date": 1529528566, "message": "some commit", "parents": ["30443c40415321c0157d3798f14c51068edb428d"], "bookmarks": [] },
  >       { "node": "773bd8234d94c44079b4409525028517fcbd98ba", "phase": "draft", "author": "Test User", "date": 1532525470, "message": "some commit", "parents": ["c609e6238e05accd090222c74a0699238f394ba4"], "bookmarks": ["somebookmark"] },
  >       { "node": "99d5fb5998e4f0a77a6b867ddeee93e7666e76c6", "phase": "public", "author": "Test User", "date": 1531229959, "message": "some commit", "parents": ["0c9fb09820a8fecb7ca9f5c46a776b72ffe41f24"], "bookmarks": [] },
  >       { "node": "aa84f0443f949a6accca6d67b2790d2f37927451", "phase": "draft", "author": "Test User", "date": 1531249011, "message": "some commit", "parents": ["99d5fb5998e4f0a77a6b867ddeee93e7666e76c6"], "bookmarks": [] },
  >       { "node": "c609e6238e05accd090222c74a0699238f394ba4", "phase": "public", "author": "Test User", "date": 1532342212, "message": "some commit", "parents": ["8cf8e2da24fdedf8f90276663bf8bb8acf60af2d"], "bookmarks": [] }]
  >   }
  > }
  > EOF

  $ hg cloud log
  the repository is not connected to any workspace, assuming the 'default' workspace
  commit:      773bd8234d94
  user:        Test User
  date:        Wed Jul 25 13:31:10 2018 +0000
  summary:     some commit
  
  commit:      685a62272258
  user:        Test User
  date:        Thu Jul 12 15:20:04 2018 +0000
  summary:     some commit
  
  commit:      aa84f0443f94
  user:        Test User
  date:        Tue Jul 10 18:56:51 2018 +0000
  summary:     some commit
  
  commit:      717dccd1a732
  user:        Test User
  date:        Wed Jun 20 21:02:46 2018 +0000
  summary:     some commit
  
  commit:      0067e44d36d9
  user:        Test User
  date:        Tue May 29 20:36:25 2018 +0000
  summary:     some commit


  $ hg cloud log -T "{node}: {date}\n"
  the repository is not connected to any workspace, assuming the 'default' workspace
  773bd8234d94c44079b4409525028517fcbd98ba: 15325254700
  685a62272258b3bd4d71ac0b331486276b3c2599: 15314088040
  aa84f0443f949a6accca6d67b2790d2f37927451: 15312490110
  717dccd1a732f794c51df27f7ba143c5c743d770: 15295285660
  0067e44d36d919bec1bff6ac65d277e8e0dc2250: 15276261850


  $ hg cloud log -T "{node}: {desc|firstline}\n"
  the repository is not connected to any workspace, assuming the 'default' workspace
  773bd8234d94c44079b4409525028517fcbd98ba: some commit
  685a62272258b3bd4d71ac0b331486276b3c2599: some commit
  aa84f0443f949a6accca6d67b2790d2f37927451: some commit
  717dccd1a732f794c51df27f7ba143c5c743d770: some commit
  0067e44d36d919bec1bff6ac65d277e8e0dc2250: some commit

  $ hg cloud log -T "{node}: {phase}\n"
  the repository is not connected to any workspace, assuming the 'default' workspace
  773bd8234d94c44079b4409525028517fcbd98ba: draft
  685a62272258b3bd4d71ac0b331486276b3c2599: draft
  aa84f0443f949a6accca6d67b2790d2f37927451: draft
  717dccd1a732f794c51df27f7ba143c5c743d770: draft
  0067e44d36d919bec1bff6ac65d277e8e0dc2250: draft

  $ hg cloud log -T "{node} {bookmarks}\n"
  the repository is not connected to any workspace, assuming the 'default' workspace
  773bd8234d94c44079b4409525028517fcbd98ba somebookmark
  685a62272258b3bd4d71ac0b331486276b3c2599 
  aa84f0443f949a6accca6d67b2790d2f37927451 
  717dccd1a732f794c51df27f7ba143c5c743d770 
  0067e44d36d919bec1bff6ac65d277e8e0dc2250 

  $ hg cloud log --verbose -l 3
  the repository is not connected to any workspace, assuming the 'default' workspace
  commit:      773bd8234d94
  user:        Test User
  date:        Wed Jul 25 13:31:10 2018 +0000
  description:
  some commit
  
  
  commit:      685a62272258
  user:        Test User
  date:        Thu Jul 12 15:20:04 2018 +0000
  description:
  some commit
  
  
  commit:      aa84f0443f94
  user:        Test User
  date:        Tue Jul 10 18:56:51 2018 +0000
  description:
  some commit

Test with date range spec
  $ hg cloud log -d "jul 19 2018 to aug 2018" -T "{node}: {date(date, '%Y-%m-%d')}\n"
  the repository is not connected to any workspace, assuming the 'default' workspace
  773bd8234d94c44079b4409525028517fcbd98ba: 2018-07-25
  
  $ hg cloud log -d "jul 10 2018 to aug 2018" -T "{node}: {date(date, '%Y-%m-%d')}\n"
  the repository is not connected to any workspace, assuming the 'default' workspace
  773bd8234d94c44079b4409525028517fcbd98ba: 2018-07-25
  685a62272258b3bd4d71ac0b331486276b3c2599: 2018-07-12
  aa84f0443f949a6accca6d67b2790d2f37927451: 2018-07-10

  $ hg cloud log -d "jul 10 2018 to aug 2018" -T "{node}: {date(date, '%Y-%m-%d')}\n" -l 1
  the repository is not connected to any workspace, assuming the 'default' workspace
  773bd8234d94c44079b4409525028517fcbd98ba: 2018-07-25
