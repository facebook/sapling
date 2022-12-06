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

Tests for hg cloud sl --date "2019-06-23 19:34:39"
  $ cat > $TESTTMP/usersmartlogbyversiondata << EOF
  > {
  >   "smartlog": {
  >     "nodes": []
  >   }
  > }
  > EOF
  $ hg cloud sl --date "2019-06-23 19:34:39"
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog version 42 
  synced at 2019-07-09 16:46:27
  $ cat > $TESTTMP/usersmartlogbyversiondata << EOF
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

  $ hg cloud sl --date "2019-06-23 19:34:39"
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog version 42 
  synced at 2019-07-09 16:46:27
  
    o  773bd8  Test User 2018-07-25 13:31 +0000 somebookmark
  ╭─╯  some commit
  │
  o  c609e6 (public)  2018-07-23 10:36 +0000
  ╷  some commit
  ╷
  ╷ o  685a62  Test User 2018-07-12 15:20 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  aa84f0  Test User 2018-07-10 18:56 +0000
  ╭─╯  some commit
  │
  o  99d5fb (public)  2018-07-10 13:39 +0000
  ╷  some commit
  ╷
  ╷ o  717dcc  Test User 2018-06-20 21:02 +0000
  ╭─╯  some commit
  │
  o  30443c (public)  2018-06-20 16:21 +0000
  ╷  some commit
  ╷
  ╷ o  0067e4  Test User 2018-05-29 20:36 +0000
  ╭─╯  some commit
  │
  o  4b1141 (public)  2018-05-29 20:23 +0000
     some commit
  $ hg cloud sl --date "2019-06-23 19:34:"
  the repository is not connected to any workspace, assuming the 'default' workspace
  hg: parse error: invalid date: '2019-06-23 19:34:'
  [255]
Tests for hg cloud sl --workspace-version
  $ cat > $TESTTMP/usersmartlogbyversiondata << EOF
  > {
  >   "smartlog": {
  >     "nodes": []
  >   }
  > }
  > EOF
  $ hg cloud sl --workspace-version 42
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog version 42 
  synced at 2019-07-09 16:46:27

  $ cat > $TESTTMP/usersmartlogbyversiondata << EOF
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

  $ hg cloud sl --workspace-version 42
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog version 42 
  synced at 2019-07-09 16:46:27
  
    o  773bd8  Test User 2018-07-25 13:31 +0000 somebookmark
  ╭─╯  some commit
  │
  o  c609e6 (public)  2018-07-23 10:36 +0000
  ╷  some commit
  ╷
  ╷ o  685a62  Test User 2018-07-12 15:20 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  aa84f0  Test User 2018-07-10 18:56 +0000
  ╭─╯  some commit
  │
  o  99d5fb (public)  2018-07-10 13:39 +0000
  ╷  some commit
  ╷
  ╷ o  717dcc  Test User 2018-06-20 21:02 +0000
  ╭─╯  some commit
  │
  o  30443c (public)  2018-06-20 16:21 +0000
  ╷  some commit
  ╷
  ╷ o  0067e4  Test User 2018-05-29 20:36 +0000
  ╭─╯  some commit
  │
  o  4b1141 (public)  2018-05-29 20:23 +0000
     some commit


