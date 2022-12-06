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

Tests for hg cloud sl
  $ cat > $TESTTMP/usersmartlogdata << EOF
  > {
  >   "smartlog": {
  >     "nodes": []
  >   }
  > }
  > EOF
  $ hg cloud sl
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog:

  $ cat > $TESTTMP/usersmartlogdata << EOF
  > {
  >   "smartlog": {
  >     "nodes": [
  >       { "node": "0c7afd97b15baf854aca030aa39bda0f48df03b4", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["2bb232ccf29f52a20aa1b91bdd4bc8d3823321e6"], "bookmarks": [] },
  >       { "node": "0c8dbcc48985c25978c34325c0a2335f89285a25", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["3b2c64ad87a575cda3ca57adc2effaae11d7f3e3"], "bookmarks": [] },
  >       { "node": "1114f660376607695319798d7d8753cf76f19180", "phase": "draft", "author": "Test User", "date": 1532033634, "message": "some commit", "parents": ["ccbc7079cd6880b49ab86753f0a08c40b9b8c62a"], "bookmarks": [] },
  >       { "node": "193082e2737537de45c069027e29fc271cc836d8", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["ea72aa3842a8e034360485fcf5520e7c7fb38eac"], "bookmarks": [] },
  >       { "node": "1971565b3bdb11db79fc9323e11cadc64a8e7ca7", "phase": "draft", "author": "Test User", "date": 1532034884, "message": "some commit", "parents": ["f311da13f93e0f6305dc1f20457f745e09553865"], "bookmarks": [] },
  >       { "node": "277a2f3fbe422c5ab85f4458f87a8ec7b11c56ab", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["0c7afd97b15baf854aca030aa39bda0f48df03b4"], "bookmarks": [] },
  >       { "node": "2bb232ccf29f52a20aa1b91bdd4bc8d3823321e6", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["aefc186211e5d494fe41c9cbad0203f95479a0e0"], "bookmarks": [] },
  >       { "node": "2c039cf40a3a7ebd4f0a941734142ae7f72b4b9a", "phase": "public", "author": "Test User", "date": 1532017245, "message": "some commit", "parents": ["e3a545b03d3d5341ff89dc1ab870e40d13773cac"], "bookmarks": [] },
  >       { "node": "334a425ad55e33368adf7f2782ee84fd28f03892", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["63d3b01305e67aa3dbdc881cb5a26f56af8455ac"], "bookmarks": [] },
  >       { "node": "35ca84092ea555e0c93adb6a27ed9d6968f6926f", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["669c3386e674f31290871a43d0b1d7743826891e"], "bookmarks": [] },
  >       { "node": "3b2c64ad87a575cda3ca57adc2effaae11d7f3e3", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["f6c426a8f63babd33959d88142dadc4a5d7eff84"], "bookmarks": [] },
  >       { "node": "3ca9f5f4cd52393f1f0acb107095deec5d24bc5e", "phase": "public", "author": "Test User", "date": 1531429003, "message": "some commit", "parents": ["97e442c38e0b59325627e2e4ae76393692cea784"], "bookmarks": [] },
  >       { "node": "44b320a861769f715c06c0a14264d39fe42add1e", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["f238a8a93cbe0b3e31ef453778921fb98c502634"], "bookmarks": [] },
  >       { "node": "4b8c26bb64bf922bafdcb316d0446f59c8a508f9", "phase": "draft", "author": "Test User", "date": 1531495558, "message": "some commit", "parents": ["b46364100b093e173db3098136f495417b36c939"], "bookmarks": [] },
  >       { "node": "4bebf9ad03c362080f78964edc3bba8c642c6ffd", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["e248c65f92e1be7755fc944cef2923919d1e467d"], "bookmarks": [] },
  >       { "node": "4c0aabbc50ced3d2f89008ac88ea69643a435c2c", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["bdaee4809bbf4ff5b32f7a3fc51860e91eca86dd"], "bookmarks": [] },
  >       { "node": "5100bc57fbedd611444cc321109db5940f1599e4", "phase": "draft", "author": "Test User", "date": 1531943080, "message": "some commit", "parents": ["bdd634cdd6c522bb714026a5f6d27b8f0030a924"], "bookmarks": [] },
  >       { "node": "51ff4ab4fb3425431a9cb0d467905c28e57fe9f5", "phase": "draft", "author": "Test User", "date": 1532033179, "message": "some commit", "parents": ["efb6f78a35b68b74f95873c9c90c9f406c67704e"], "bookmarks": [] },
  >       { "node": "53d3b50752db0c642f38173fbe6d00dcad1b3815", "phase": "draft", "author": "Test User", "date": 1532033634, "message": "some commit", "parents": ["fcd54188dd1902d56aa82b001b70e9a01d6a1e78"], "bookmarks": [] },
  >       { "node": "5ef0df08a074752b6f8b34c9b23bf8a363ab1b95", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["b02028018586a1a94d12c1721dac736f38fd770d"], "bookmarks": [] },
  >       { "node": "63d3b01305e67aa3dbdc881cb5a26f56af8455ac", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["193082e2737537de45c069027e29fc271cc836d8"], "bookmarks": [] },
  >       { "node": "669c3386e674f31290871a43d0b1d7743826891e", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["f5428e3955a9be325fd962320c5a6ea4fa241355"], "bookmarks": [] },
  >       { "node": "67dd6833f29dd3540842ddbf6e178fd09dec6b7f", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["1114f660376607695319798d7d8753cf76f19180"], "bookmarks": [] },
  >       { "node": "72bcf5a053ef6cabe53184fc346bfd54e76be977", "phase": "public", "author": "Test User", "date": 1531849212, "message": "some commit", "parents": ["45f73e78e2d611dab81cd0442b22279a45586c1a"], "bookmarks": [] },
  >       { "node": "73238e4b9b680ab720ca1889a41be9f5299d9be4", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["44b320a861769f715c06c0a14264d39fe42add1e"], "bookmarks": [] },
  >       { "node": "786c1d34e1680ca5938925ec516206d123a70d0f", "phase": "draft", "author": "Test User", "date": 1527098528, "message": "some commit", "parents": ["7e1ae2528e025754968e82feef3b2976a37add4c"], "bookmarks": [] },
  >       { "node": "7c0a619b263cc229d9a43fc7a9b75ab083900ace", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["334a425ad55e33368adf7f2782ee84fd28f03892"], "bookmarks": [] },
  >       { "node": "7e1ae2528e025754968e82feef3b2976a37add4c", "phase": "public", "author": "Test User", "date": 1527094285, "message": "some commit", "parents": ["1cc0dd4e5697a33c595151da9a311bb5f3c14019"], "bookmarks": [] },
  >       { "node": "8342640303e217711a4c686e6d9a6b6d579a1ff9", "phase": "draft", "author": "Test User", "date": 1527098530, "message": "some commit", "parents": ["7e1ae2528e025754968e82feef3b2976a37add4c"], "bookmarks": [] },
  >       { "node": "89e915fc5f8183123c6ce4d2cd2d7267cc892677", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["4bebf9ad03c362080f78964edc3bba8c642c6ffd"], "bookmarks": [] },
  >       { "node": "8a486e8319907ff6ff50407ee44f617089096f0a", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["f311da13f93e0f6305dc1f20457f745e09553865"], "bookmarks": [] },
  >       { "node": "8ccfbe34933a2b69493a312e79f0d35ed5136749", "phase": "draft", "author": "Test User", "date": 1531857514, "message": "some commit", "parents": ["72bcf5a053ef6cabe53184fc346bfd54e76be977"], "bookmarks": [] },
  >       { "node": "907757f898f59763b390f2f12965e98f5ecaaeb9", "phase": "draft", "author": "Test User", "date": 1532029518, "message": "some commit", "parents": ["2c039cf40a3a7ebd4f0a941734142ae7f72b4b9a"], "bookmarks": [] },
  >       { "node": "a54c7029c39f93a0d3fad69dc0637b5ceab3ba9a", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["0c8dbcc48985c25978c34325c0a2335f89285a25"], "bookmarks": [] },
  >       { "node": "ac628c7a8a3adee1095b938d15661cd8443ed2e8", "phase": "draft", "author": "Test User", "date": 1528216560, "message": "some commit", "parents": ["e0a8504879f0d980d0d8f1fd61cbbcbe13894e1b"], "bookmarks": [] },
  >       { "node": "ae5e6fbf7611bfd7ecf895882af7d7ab74fcf0b2", "phase": "draft", "author": "Test User", "date": 1530207952, "message": "some commit", "parents": ["ff1ae4d4ded3a8632b865c8d8076e1b25ff87325"], "bookmarks": [] },
  >       { "node": "aefc186211e5d494fe41c9cbad0203f95479a0e0", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["7c0a619b263cc229d9a43fc7a9b75ab083900ace"], "bookmarks": [] },
  >       { "node": "b02028018586a1a94d12c1721dac736f38fd770d", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["8a486e8319907ff6ff50407ee44f617089096f0a"], "bookmarks": [] },
  >       { "node": "b46364100b093e173db3098136f495417b36c939", "phase": "draft", "author": "Test User", "date": 1531495552, "message": "some commit", "parents": ["3ca9f5f4cd52393f1f0acb107095deec5d24bc5e"], "bookmarks": [] },
  >       { "node": "b4a0b0873676e6aea090b1e6d3a6e55398eb4b94", "phase": "draft", "author": "Test User", "date": 1532036821, "message": "some commit", "parents": ["67dd6833f29dd3540842ddbf6e178fd09dec6b7f"], "bookmarks": [] },
  >       { "node": "b8d4ca8db66710e60942d6ce8081f40e3e99447f", "phase": "draft", "author": "Test User", "date": 1532041424, "message": "some commit", "parents": ["b4a0b0873676e6aea090b1e6d3a6e55398eb4b94"], "bookmarks": [] },
  >       { "node": "bdaee4809bbf4ff5b32f7a3fc51860e91eca86dd", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["d849186124da33f7f6dbba9ed87d7171ac766bbc"], "bookmarks": [] },
  >       { "node": "bdd634cdd6c522bb714026a5f6d27b8f0030a924", "phase": "public", "author": "Test User", "date": 1531770310, "message": "some commit", "parents": ["04f3024700906d47ce601f399199792d6ce4abd9"], "bookmarks": [] },
  >       { "node": "ccbc7079cd6880b49ab86753f0a08c40b9b8c62a", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["73238e4b9b680ab720ca1889a41be9f5299d9be4"], "bookmarks": [] },
  >       { "node": "d78d6eca8dc7ff3c4841973fc6cad26384965b55", "phase": "draft", "author": "Test User", "date": 1527098529, "message": "some commit", "parents": ["7e1ae2528e025754968e82feef3b2976a37add4c"], "bookmarks": [] },
  >       { "node": "d849186124da33f7f6dbba9ed87d7171ac766bbc", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["277a2f3fbe422c5ab85f4458f87a8ec7b11c56ab"], "bookmarks": [] },
  >       { "node": "e0a8504879f0d980d0d8f1fd61cbbcbe13894e1b", "phase": "public", "author": "Test User", "date": 1528216301, "message": "some commit", "parents": ["06fc22417a714c01680e6cb55ab8bd525d24962b"], "bookmarks": [] },
  >       { "node": "e248c65f92e1be7755fc944cef2923919d1e467d", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["a54c7029c39f93a0d3fad69dc0637b5ceab3ba9a"], "bookmarks": [] },
  >       { "node": "ea72aa3842a8e034360485fcf5520e7c7fb38eac", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["5ef0df08a074752b6f8b34c9b23bf8a363ab1b95"], "bookmarks": [] },
  >       { "node": "efb6f78a35b68b74f95873c9c90c9f406c67704e", "phase": "public", "author": "Test User", "date": 1532031362, "message": "some commit", "parents": ["5e81c12031855be42eb59a8bb50d2f1f77a3f194"], "bookmarks": [] },
  >       { "node": "f238a8a93cbe0b3e31ef453778921fb98c502634", "phase": "draft", "author": "Test User", "date": 1532034884, "message": "some commit", "parents": ["1971565b3bdb11db79fc9323e11cadc64a8e7ca7"], "bookmarks": [] },
  >       { "node": "f311da13f93e0f6305dc1f20457f745e09553865", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["51ff4ab4fb3425431a9cb0d467905c28e57fe9f5"], "bookmarks": [] },
  >       { "node": "f5428e3955a9be325fd962320c5a6ea4fa241355", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["4c0aabbc50ced3d2f89008ac88ea69643a435c2c"], "bookmarks": [] },
  >       { "node": "f6c426a8f63babd33959d88142dadc4a5d7eff84", "phase": "draft", "author": "Test User", "date": 1532033633, "message": "some commit", "parents": ["35ca84092ea555e0c93adb6a27ed9d6968f6926f"], "bookmarks": [] },
  >       { "node": "fcd54188dd1902d56aa82b001b70e9a01d6a1e78", "phase": "draft", "author": "Test User", "date": 1532033634, "message": "some commit", "parents": ["89e915fc5f8183123c6ce4d2cd2d7267cc892677"], "bookmarks": [] },
  >       { "node": "ff1ae4d4ded3a8632b865c8d8076e1b25ff87325", "phase": "public", "author": "Test User", "date": 1530207381, "message": "some commit", "parents": ["8e0c23262639ba260898b03cd2aecc8b0a4cdc87"], "bookmarks": [] }]
  >   }
  > }
  > EOF

  $ hg cloud sl
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog:
  
    o  53d3b5  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  fcd541  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  89e915  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  4bebf9  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  e248c6  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  a54c70  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  0c8dbc  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  3b2c64  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  f6c426  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  35ca84  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  669c33  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  f5428e  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  4c0aab  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  bdaee4  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  d84918  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  277a2f  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  0c7afd  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  2bb232  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  aefc18  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  7c0a61  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  334a42  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  63d3b0  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  193082  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  ea72aa  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  5ef0df  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  b02028  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  8a486e  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    │ o  b8d4ca  Test User 2018-07-19 23:03 +0000
    │ │  some commit
    │ │
    │ o  b4a0b0  Test User 2018-07-19 21:47 +0000
    │ │  some commit
    │ │
    │ o  67dd68  Test User 2018-07-19 20:53 +0000
    │ │  some commit
    │ │
    │ o  1114f6  Test User 2018-07-19 20:53 +0000
    │ │  some commit
    │ │
    │ o  ccbc70  Test User 2018-07-19 20:53 +0000
    │ │  some commit
    │ │
    │ o  73238e  Test User 2018-07-19 20:53 +0000
    │ │  some commit
    │ │
    │ o  44b320  Test User 2018-07-19 20:53 +0000
    │ │  some commit
    │ │
    │ o  f238a8  Test User 2018-07-19 21:14 +0000
    │ │  some commit
    │ │
    │ o  197156  Test User 2018-07-19 21:14 +0000
    ├─╯  some commit
    │
    o  f311da  Test User 2018-07-19 20:53 +0000
    │  some commit
    │
    o  51ff4a  Test User 2018-07-19 20:46 +0000
  ╭─╯  some commit
  │
  o  efb6f7 (public)  2018-07-19 20:16 +0000
  ╷  some commit
  ╷
  ╷ o  907757  Test User 2018-07-19 19:45 +0000
  ╭─╯  some commit
  │
  o  2c039c (public)  2018-07-19 16:20 +0000
  ╷  some commit
  ╷
  ╷ o  8ccfbe  Test User 2018-07-17 19:58 +0000
  ╭─╯  some commit
  │
  o  72bcf5 (public)  2018-07-17 17:40 +0000
  ╷  some commit
  ╷
  ╷ o  5100bc  Test User 2018-07-18 19:44 +0000
  ╭─╯  some commit
  │
  o  bdd634 (public)  2018-07-16 19:45 +0000
  ╷  some commit
  ╷
  ╷ o  4b8c26  Test User 2018-07-13 15:25 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b46364  Test User 2018-07-13 15:25 +0000
  ╭─╯  some commit
  │
  o  3ca9f5 (public)  2018-07-12 20:56 +0000
  ╷  some commit
  ╷
  ╷ o  ae5e6f  Test User 2018-06-28 17:45 +0000
  ╭─╯  some commit
  │
  o  ff1ae4 (public)  2018-06-28 17:36 +0000
  ╷  some commit
  ╷
  ╷ o  ac628c  Test User 2018-06-05 16:36 +0000
  ╭─╯  some commit
  │
  o  e0a850 (public)  2018-06-05 16:31 +0000
  ╷  some commit
  ╷
  ╷ o  d78d6e  Test User 2018-05-23 18:02 +0000
  ╭─╯  some commit
  │
  │ o  834264  Test User 2018-05-23 18:02 +0000
  ├─╯  some commit
  │
  │ o  786c1d  Test User 2018-05-23 18:02 +0000
  ├─╯  some commit
  │
  o  7e1ae2 (public)  2018-05-23 16:51 +0000
     some commit


  $ cat > $TESTTMP/usersmartlogdata << EOF
  > {
  >   "smartlog": {
  >     "nodes": [
  >       { "node": "05b7015f10f4da2522636d75ec210e801241111a", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["3a54e4e09637eb14f2bb4654396e3f4230cbbc4f"], "bookmarks": [] },
  >       { "node": "06b916bedf4f562203b5419f17e5cfa82270f69d", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["d965af2c7d6d779417637154403763f429e41260"], "bookmarks": [] },
  >       { "node": "083ccd603a104dd32d6715332d6ce15993039a0e", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["b020b9afce4ce44c77530b22460a1c011d0519d3"], "bookmarks": [] },
  >       { "node": "094e36e02857502db35c5e7135d667fd1e959f7e", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["ec877f5b97bb1376cf6f681a90e52efd586c1363"], "bookmarks": [] },
  >       { "node": "098b6afc685c80d0bb3a51a235f752801fb358a2", "phase": "public", "author": "Test User", "date": 1532476040, "message": "some commit", "parents": ["47096fe33c8616dfafd8ee67bd2a18939f4b3d0e"], "bookmarks": [] },
  >       { "node": "0c157bec5f5d8d815ca22afd32c8bd335779d9ae", "phase": "public", "author": "Test User", "date": 1530031998, "message": "some commit", "parents": ["178aec9241eea49a89448c718fbe48b16c4f72f2"], "bookmarks": [] },
  >       { "node": "11492a90c197843d365c76efc767162927428562", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["98994b283ec191d2a41d038fce22fca23605d766"], "bookmarks": [] },
  >       { "node": "17afae144aeb5f82802f4d2b945e57092d55ddac", "phase": "draft", "author": "Test User", "date": 1532376198, "message": "some commit", "parents": ["eb200cda025e2223c68e36191fd7e6400dc9d64d"], "bookmarks": [] },
  >       { "node": "1831786daa5cceb87cfc7ab85ed1a0b24dc89c77", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["f6d061fcd05e505912d158457251c91a65b4b29d"], "bookmarks": [] },
  >       { "node": "18611b8608410a69bdbf06dc271f4b1375a74e0a", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["05b7015f10f4da2522636d75ec210e801241111a"], "bookmarks": [] },
  >       { "node": "192d92eeab77bbfef9e0f08bfe725a5645c03da2", "phase": "draft", "author": "Test User", "date": 1531959475, "message": "some commit", "parents": ["f3692056ba107c3f57e1f83b2d3420ff4540a7d5"], "bookmarks": [] },
  >       { "node": "1f074c0f012ab3c3ab1c1b1eb94c0f893239b9f5", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["71b2a7789748567ae140f64a09b257be1029c4a3"], "bookmarks": [] },
  >       { "node": "1f4c3f90f5ea329d03e31f9626623e301abdeff9", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["36662653cc7be1e9e97e1b88fd8a29db52c991b9"], "bookmarks": [] },
  >       { "node": "1fe6c7eb010d0c320180c19f39536549c7c9ffcc", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["445c23e8660cd9d2b6c9c17bdc88080ac5864098"], "bookmarks": [] },
  >       { "node": "206df520227c87eb3708180f0cd3e939017ee125", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["758c972e255905dbc48a7c624459334f5a9bcab9"], "bookmarks": [] },
  >       { "node": "20917aedd9b94ce35bfe41c5a531c2acaa00d829", "phase": "draft", "author": "Test User", "date": 1530143221, "message": "some commit", "parents": ["c64f72e64728033d6e57684ca6f4e147f795a903"], "bookmarks": [] },
  >       { "node": "244df61c42d2e4dd523f5b40c2b1badb49109bb7", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["b1204f43d7179ae2c97fd4786ab74504d42fe688"], "bookmarks": [] },
  >       { "node": "2582d5bb31b0bb03ec9e3a9860a07429f3a911b8", "phase": "draft", "author": "Test User", "date": 1530642445, "message": "some commit", "parents": ["5dd5b0ae592c80884bca519a8e24c71ab3cd3c30"], "bookmarks": [] },
  >       { "node": "2cb52de5378f55e37b40723c5c57ce23f02cef06", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["46590aaf955c822075f5640551de43eb212a0d0b"], "bookmarks": [] },
  >       { "node": "2e32bea024ddcd174f93fc5229551cecb2278f1f", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["18611b8608410a69bdbf06dc271f4b1375a74e0a"], "bookmarks": [] },
  >       { "node": "36662653cc7be1e9e97e1b88fd8a29db52c991b9", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["11492a90c197843d365c76efc767162927428562"], "bookmarks": [] },
  >       { "node": "390b78c80f02c8e8025fe977f2be81ac9551b699", "phase": "public", "author": "Test User", "date": 1530032011, "message": "some commit", "parents": ["0c157bec5f5d8d815ca22afd32c8bd335779d9ae"], "bookmarks": [] },
  >       { "node": "39dbb5c9c1d2fc4ae9b3d4f4ce1af99c602de56b", "phase": "draft", "author": "Test User", "date": 1530144230, "message": "some commit", "parents": ["c64f72e64728033d6e57684ca6f4e147f795a903"], "bookmarks": [] },
  >       { "node": "3a54e4e09637eb14f2bb4654396e3f4230cbbc4f", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["094e36e02857502db35c5e7135d667fd1e959f7e"], "bookmarks": [] },
  >       { "node": "3a6604464a15a05b6866915b9189cec957734e38", "phase": "draft", "author": "Test User", "date": 1531958694, "message": "some commit", "parents": ["b593fd1372fa20ca712396549d8ac91358d249b9"], "bookmarks": [] },
  >       { "node": "3ac2055c4768f11f3c9f7a7f9fd00af80727ffca", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["cfb3d23af913a0db81979eef9773f66c38107732"], "bookmarks": [] },
  >       { "node": "3d61c723570e06f62d37c58ffabb51e04b88d235", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["4b67b7fbc52b7b3a601f292cf8b61ecd399b87c6"], "bookmarks": [] },
  >       { "node": "41b13a0e786f9d5ab6884d5daab81ddf81493743", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["1f074c0f012ab3c3ab1c1b1eb94c0f893239b9f5"], "bookmarks": [] },
  >       { "node": "43a3ea6b02b55e6f10308570bac26806c04af3a4", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["083ccd603a104dd32d6715332d6ce15993039a0e"], "bookmarks": [] },
  >       { "node": "445c23e8660cd9d2b6c9c17bdc88080ac5864098", "phase": "public", "author": "Test User", "date": 1532406859, "message": "some commit", "parents": ["c6202f8c958510d1b659fa06b530a90877e5dab5"], "bookmarks": [] },
  >       { "node": "46590aaf955c822075f5640551de43eb212a0d0b", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["79779080aee9b7da68277b8ac01bf43c81cf10cf"], "bookmarks": [] },
  >       { "node": "485b19ad1150d489673b5acaa495278bbe45d6cf", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["86b66e1f0a21e8aaa4c20b8ae0b635959b27dc74"], "bookmarks": [] },
  >       { "node": "4b67b7fbc52b7b3a601f292cf8b61ecd399b87c6", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["56436bf259a5c11f647d26ee29d33f129a1c1212"], "bookmarks": [] },
  >       { "node": "4ccaa1f1020ab302c1820e55f3078cf9808a25f7", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["bcf951b1c7245f1b189d383fa519a0c1b9704e6f"], "bookmarks": [] },
  >       { "node": "50f17725ab57f4f68b8997a62a02dc8b52c35b99", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["9c13c13f824183699d09edc78b568f966ac63d94"], "bookmarks": [] },
  >       { "node": "545c51e84691b45fd3776aa26a49f6b0b9b155bd", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["639f1bb8c3c915e7642319fadefd892b5759140d"], "bookmarks": [] },
  >       { "node": "55a0d63d5efbaadab39e95485e1cd91851bbeb75", "phase": "public", "author": "Test User", "date": 1532371078, "message": "some commit", "parents": ["c3e10d5a2c0345f8ff8a54d55bafce455af1c76c"], "bookmarks": [] },
  >       { "node": "56436bf259a5c11f647d26ee29d33f129a1c1212", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["06b916bedf4f562203b5419f17e5cfa82270f69d"], "bookmarks": [] },
  >       { "node": "5bba72c00dedfccbb2ffedb64d8babe74782332e", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["68315b4baa83b33425f85c621c039dde03b07d14"], "bookmarks": [] },
  >       { "node": "5dd5b0ae592c80884bca519a8e24c71ab3cd3c30", "phase": "public", "author": "Test User", "date": 1530642375, "message": "some commit", "parents": ["9b925cfca5c2f5dbb814e7f16bffc27c5a385a36"], "bookmarks": [] },
  >       { "node": "5ef32b914b18aac9db961515adf8aadec43e703f", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["89c451816e0ff6eac7153b37c1bc36fda0ef73fd"], "bookmarks": [] },
  >       { "node": "5ff211e61e2081969109b9789aa38762213f4519", "phase": "draft", "author": "Test User", "date": 1531959912, "message": "some commit", "parents": ["192d92eeab77bbfef9e0f08bfe725a5645c03da2"], "bookmarks": [] },
  >       { "node": "602227539f013a03923e97c52439b5f7cf7c9622", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["3ac2055c4768f11f3c9f7a7f9fd00af80727ffca"], "bookmarks": [] },
  >       { "node": "616bf58eeab9fa8438dcbb19cc128f6213fed173", "phase": "draft", "author": "Test User", "date": 1532410887, "message": "some commit", "parents": ["d12ab83257fe4d6e572effa704955e169ea001a1"], "bookmarks": [] },
  >       { "node": "61e66ebd930b1f924f3837e4452dd1bf13e50c35", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["99509b923a42179747fa4b74aea438da999d4e49"], "bookmarks": [] },
  >       { "node": "639f1bb8c3c915e7642319fadefd892b5759140d", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["f50cdbb9581842a4245e2e1dfffff7bef621663b"], "bookmarks": [] },
  >       { "node": "65b8e626b28e20726aee106ba8e535ded0649efa", "phase": "draft", "author": "Test User", "date": 1532376198, "message": "some commit", "parents": ["b56685978251f30e054e689491ea241b9b9b1fbc"], "bookmarks": [] },
  >       { "node": "65cc49a2efdfa67e8b1652fbfb5b5e27a8fda2bd", "phase": "draft", "author": "Test User", "date": 1531957615, "message": "some commit", "parents": ["90d8c9afc3d010e5d01adb3fcaa60275163c3b28"], "bookmarks": [] },
  >       { "node": "665b9e9866500f5caa1016d290efea74755605d7", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["61e66ebd930b1f924f3837e4452dd1bf13e50c35"], "bookmarks": [] },
  >       { "node": "6792bfc0bcfd6157249ea3f6bad550c6a87bf4e8", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["ca31bbbdc721bfc8c64b39c18e6e05781c4bf266"], "bookmarks": [] },
  >       { "node": "68315b4baa83b33425f85c621c039dde03b07d14", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["b20853fb9eafaa237ad99005e9b3385d930b507b"], "bookmarks": [] },
  >       { "node": "71b2a7789748567ae140f64a09b257be1029c4a3", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["ebab1acdc243a40fe7a8e2f97ac8eb6164de6a57"], "bookmarks": [] },
  >       { "node": "758c972e255905dbc48a7c624459334f5a9bcab9", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["fadcc66210c176515e54a953fabf1fbe6cde0dc4"], "bookmarks": [] },
  >       { "node": "75b54610abbbcf6c0fe68f1ed73ffd032d3c8ade", "phase": "draft", "author": "Test User", "date": 1532376198, "message": "some commit", "parents": ["eda0bc36460f326aabe779958c6af8e0b9fb569c"], "bookmarks": [] },
  >       { "node": "78503f817f8fae6eb6c3368e835c7ff7cdf21ce4", "phase": "draft", "author": "Test User", "date": 1531959475, "message": "some commit", "parents": ["3a6604464a15a05b6866915b9189cec957734e38"], "bookmarks": [] },
  >       { "node": "79779080aee9b7da68277b8ac01bf43c81cf10cf", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["ec9300df05618ce73cf43a39d21ffdd45ab7f2cb"], "bookmarks": [] },
  >       { "node": "7c3fe941ea760cdcdd0846128ca6fd13af4d4285", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["602227539f013a03923e97c52439b5f7cf7c9622"], "bookmarks": [] },
  >       { "node": "86b66e1f0a21e8aaa4c20b8ae0b635959b27dc74", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["6792bfc0bcfd6157249ea3f6bad550c6a87bf4e8"], "bookmarks": [] },
  >       { "node": "89095bd03bff3e0a6bf0d59c9c41400c693b019c", "phase": "draft", "author": "Test User", "date": 1532376198, "message": "some commit", "parents": ["f9d07a871bfea873075db7e0a346c2c8ac2c95a8"], "bookmarks": [] },
  >       { "node": "89c451816e0ff6eac7153b37c1bc36fda0ef73fd", "phase": "public", "author": "Test User", "date": 1530222847, "message": "some commit", "parents": ["66cbab1ddd254b1e0b91232565b4d512810ba03d"], "bookmarks": [] },
  >       { "node": "90d8c9afc3d010e5d01adb3fcaa60275163c3b28", "phase": "public", "author": "Test User", "date": 1531957492, "message": "some commit", "parents": ["28c40f41193eda07b7551f2a07bb5a6384c35c32"], "bookmarks": [] },
  >       { "node": "932176328c8636f9a90ecd43886120c79a8547ff", "phase": "draft", "author": "Test User", "date": 1532374994, "message": "some commit", "parents": ["dfbb040f7d1799aa1e01fcb380e790233d533029"], "bookmarks": [] },
  >       { "node": "944f52113df1197d1e974e5528daf172c254549e", "phase": "public", "author": "Test User", "date": 1530133537, "message": "some commit", "parents": ["e1d85b4ee766877c05f8a8223c40fe83e11339ee"], "bookmarks": [] },
  >       { "node": "953880aa5477cf0b46a6bc56bff4f7f89cd6af63", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["b6ac8ed4794fdf736b12a40e9c4c64301e5f1854"], "bookmarks": [] },
  >       { "node": "95453e02bcfa92f0b0b410f50ec34823bba58000", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["acdbe0a44fdef8c1ac986ef53f60887e61cbbf2d"], "bookmarks": [] },
  >       { "node": "98994b283ec191d2a41d038fce22fca23605d766", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["dc22fb6396c88b45ea8a15e3cf5e81b0fec27847"], "bookmarks": [] },
  >       { "node": "99509b923a42179747fa4b74aea438da999d4e49", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["2cb52de5378f55e37b40723c5c57ce23f02cef06"], "bookmarks": [] },
  >       { "node": "9ae2eb330b5627e0bd40daf08c0145960d25aaae", "phase": "draft", "author": "Test User", "date": 1532376198, "message": "some commit", "parents": ["75b54610abbbcf6c0fe68f1ed73ffd032d3c8ade"], "bookmarks": [] },
  >       { "node": "9c13c13f824183699d09edc78b568f966ac63d94", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["ee3bd8ce7ef5bfd397b4e7ee01070ed1ee1db519"], "bookmarks": [] },
  >       { "node": "9d121dfaee88f921441d3e440869aba294d5f0f4", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["cf25e1ab2c9b074defc414c6e36d90435501b2f3"], "bookmarks": [] },
  >       { "node": "a310c08e8178993d88045fe43f892562298cf039", "phase": "draft", "author": "Test User", "date": 1530135054, "message": "some commit", "parents": ["944f52113df1197d1e974e5528daf172c254549e"], "bookmarks": [] },
  >       { "node": "acdbe0a44fdef8c1ac986ef53f60887e61cbbf2d", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["206df520227c87eb3708180f0cd3e939017ee125"], "bookmarks": [] },
  >       { "node": "ae84d576c76db4b4a002b40aee1055c80b23867b", "phase": "public", "author": "Test User", "date": 1529949088, "message": "some commit", "parents": ["874271456dc17842c573ecabf256bae40387ea9c"], "bookmarks": [] },
  >       { "node": "b01f752a5bfd939f71b6b5915a707d06acbca5e2", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["485b19ad1150d489673b5acaa495278bbe45d6cf"], "bookmarks": [] },
  >       { "node": "b020b9afce4ce44c77530b22460a1c011d0519d3", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["95453e02bcfa92f0b0b410f50ec34823bba58000"], "bookmarks": [] },
  >       { "node": "b1204f43d7179ae2c97fd4786ab74504d42fe688", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["f07c0cad4c43bb5e1da0a10c4e74a33138bb20b9"], "bookmarks": [] },
  >       { "node": "b20853fb9eafaa237ad99005e9b3385d930b507b", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["1f4c3f90f5ea329d03e31f9626623e301abdeff9"], "bookmarks": [] },
  >       { "node": "b4b391aa99bedd19925f6ecc677adb0a73527a55", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["e6271266c9ce6c8e82f22582c6e87304ca03054d"], "bookmarks": [] },
  >       { "node": "b56685978251f30e054e689491ea241b9b9b1fbc", "phase": "draft", "author": "Test User", "date": 1532376198, "message": "some commit", "parents": ["89095bd03bff3e0a6bf0d59c9c41400c693b019c"], "bookmarks": [] },
  >       { "node": "b593fd1372fa20ca712396549d8ac91358d249b9", "phase": "draft", "author": "Test User", "date": 1531958213, "message": "some commit", "parents": ["65cc49a2efdfa67e8b1652fbfb5b5e27a8fda2bd"], "bookmarks": [] },
  >       { "node": "b6ac8ed4794fdf736b12a40e9c4c64301e5f1854", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["7c3fe941ea760cdcdd0846128ca6fd13af4d4285"], "bookmarks": [] },
  >       { "node": "b71712156e7e91d602a8cd0cd621d841f0c6968f", "phase": "draft", "author": "Test User", "date": 1530138040, "message": "some commit", "parents": ["0c157bec5f5d8d815ca22afd32c8bd335779d9ae"], "bookmarks": [] },
  >       { "node": "bb57a885b145ccbd6cee582235ee24734bc3ad08", "phase": "draft", "author": "Test User", "date": 1532376192, "message": "some commit", "parents": ["b01f752a5bfd939f71b6b5915a707d06acbca5e2"], "bookmarks": [] },
  >       { "node": "bcf951b1c7245f1b189d383fa519a0c1b9704e6f", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["9d121dfaee88f921441d3e440869aba294d5f0f4"], "bookmarks": [] },
  >       { "node": "c012a1d6b5f63ac8e6f6039ba165aba966b3fea1", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["5ef32b914b18aac9db961515adf8aadec43e703f"], "bookmarks": [] },
  >       { "node": "c64f72e64728033d6e57684ca6f4e147f795a903", "phase": "public", "author": "Test User", "date": 1530045945, "message": "some commit", "parents": ["5d72a6b12e1e052c222d3953df58e092b2c9b249"], "bookmarks": [] },
  >       { "node": "ca31bbbdc721bfc8c64b39c18e6e05781c4bf266", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["4ccaa1f1020ab302c1820e55f3078cf9808a25f7"], "bookmarks": [] },
  >       { "node": "cf25e1ab2c9b074defc414c6e36d90435501b2f3", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["953880aa5477cf0b46a6bc56bff4f7f89cd6af63"], "bookmarks": [] },
  >       { "node": "cfb3d23af913a0db81979eef9773f66c38107732", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["50f17725ab57f4f68b8997a62a02dc8b52c35b99"], "bookmarks": [] },
  >       { "node": "d12ab83257fe4d6e572effa704955e169ea001a1", "phase": "draft", "author": "Test User", "date": 1532407258, "message": "some commit", "parents": ["3d61c723570e06f62d37c58ffabb51e04b88d235"], "bookmarks": [] },
  >       { "node": "d1de39e5802e03127315ae72c77c2f5ebdcd2668", "phase": "draft", "author": "Test User", "date": 1529953922, "message": "some commit", "parents": ["ae84d576c76db4b4a002b40aee1055c80b23867b"], "bookmarks": [] },
  >       { "node": "d1f01a20c042ed0cf4829b679899254ac898efb4", "phase": "draft", "author": "Test User", "date": 1530138434, "message": "some commit", "parents": ["390b78c80f02c8e8025fe977f2be81ac9551b699"], "bookmarks": [] },
  >       { "node": "d7c29883db0d40264e3ae42b1ecfd14968123591", "phase": "draft", "author": "Test User", "date": 1532476694, "message": "some commit", "parents": ["098b6afc685c80d0bb3a51a235f752801fb358a2"], "bookmarks": [] },
  >       { "node": "d7de7d7b2e97f3883e1fdba3fd276917894acb63", "phase": "draft", "author": "Test User", "date": 1530135054, "message": "some commit", "parents": ["a310c08e8178993d88045fe43f892562298cf039"], "bookmarks": [] },
  >       { "node": "d911df3bba3da335419894bb6cbae71dd7c07f8b", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["dce5e202e47a30fa8cdcd086a01baaea14fd1406"], "bookmarks": [] },
  >       { "node": "d965af2c7d6d779417637154403763f429e41260", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["665b9e9866500f5caa1016d290efea74755605d7"], "bookmarks": [] },
  >       { "node": "dc22fb6396c88b45ea8a15e3cf5e81b0fec27847", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["41b13a0e786f9d5ab6884d5daab81ddf81493743"], "bookmarks": [] },
  >       { "node": "dce5e202e47a30fa8cdcd086a01baaea14fd1406", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["244df61c42d2e4dd523f5b40c2b1badb49109bb7"], "bookmarks": [] },
  >       { "node": "dfbb040f7d1799aa1e01fcb380e790233d533029", "phase": "draft", "author": "Test User", "date": 1532374994, "message": "some commit", "parents": ["55a0d63d5efbaadab39e95485e1cd91851bbeb75"], "bookmarks": [] },
  >       { "node": "e6271266c9ce6c8e82f22582c6e87304ca03054d", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["1fe6c7eb010d0c320180c19f39536549c7c9ffcc"], "bookmarks": [] },
  >       { "node": "eb200cda025e2223c68e36191fd7e6400dc9d64d", "phase": "draft", "author": "Test User", "date": 1532376198, "message": "some commit", "parents": ["f5cfc156383ca29ecf65bc5ccdbd02254a79f8ed"], "bookmarks": [] },
  >       { "node": "ebab1acdc243a40fe7a8e2f97ac8eb6164de6a57", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["2e32bea024ddcd174f93fc5229551cecb2278f1f"], "bookmarks": [] },
  >       { "node": "ec877f5b97bb1376cf6f681a90e52efd586c1363", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["c012a1d6b5f63ac8e6f6039ba165aba966b3fea1"], "bookmarks": [] },
  >       { "node": "ec9300df05618ce73cf43a39d21ffdd45ab7f2cb", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["545c51e84691b45fd3776aa26a49f6b0b9b155bd"], "bookmarks": [] },
  >       { "node": "eda0bc36460f326aabe779958c6af8e0b9fb569c", "phase": "draft", "author": "Test User", "date": 1532376198, "message": "some commit", "parents": ["65b8e626b28e20726aee106ba8e535ded0649efa"], "bookmarks": [] },
  >       { "node": "ee3bd8ce7ef5bfd397b4e7ee01070ed1ee1db519", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["d911df3bba3da335419894bb6cbae71dd7c07f8b"], "bookmarks": [] },
  >       { "node": "f07c0cad4c43bb5e1da0a10c4e74a33138bb20b9", "phase": "draft", "author": "Test User", "date": 1532375118, "message": "some commit", "parents": ["1831786daa5cceb87cfc7ab85ed1a0b24dc89c77"], "bookmarks": [] },
  >       { "node": "f3692056ba107c3f57e1f83b2d3420ff4540a7d5", "phase": "draft", "author": "Test User", "date": 1531959475, "message": "some commit", "parents": ["78503f817f8fae6eb6c3368e835c7ff7cdf21ce4"], "bookmarks": [] },
  >       { "node": "f50cdbb9581842a4245e2e1dfffff7bef621663b", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["43a3ea6b02b55e6f10308570bac26806c04af3a4"], "bookmarks": [] },
  >       { "node": "f50dc442c02008acfdeb922ad356cebed0aed0ef", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["b4b391aa99bedd19925f6ecc677adb0a73527a55"], "bookmarks": [] },
  >       { "node": "f5cfc156383ca29ecf65bc5ccdbd02254a79f8ed", "phase": "draft", "author": "Test User", "date": 1532376198, "message": "some commit", "parents": ["bb57a885b145ccbd6cee582235ee24734bc3ad08"], "bookmarks": [] },
  >       { "node": "f6d061fcd05e505912d158457251c91a65b4b29d", "phase": "draft", "author": "Test User", "date": 1532374994, "message": "some commit", "parents": ["932176328c8636f9a90ecd43886120c79a8547ff"], "bookmarks": [] },
  >       { "node": "f9d07a871bfea873075db7e0a346c2c8ac2c95a8", "phase": "draft", "author": "Test User", "date": 1532376198, "message": "some commit", "parents": ["17afae144aeb5f82802f4d2b945e57092d55ddac"], "bookmarks": [] },
  >       { "node": "fadcc66210c176515e54a953fabf1fbe6cde0dc4", "phase": "draft", "author": "Test User", "date": 1532407016, "message": "some commit", "parents": ["f50dc442c02008acfdeb922ad356cebed0aed0ef"], "bookmarks": [] },
  >       { "node": "fda58c71deb73bafa2a1f0bb58b638a239210925", "phase": "draft", "author": "Test User", "date": 1530222916, "message": "some commit", "parents": ["5bba72c00dedfccbb2ffedb64d8babe74782332e"], "bookmarks": [] }]
  >   }
  > }
  > EOF

  $ hg cloud sl
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog:
  
    o  d7c298  Test User 2018-07-24 23:58 +0000
  ╭─╯  some commit
  │
  o  098b6a (public)  2018-07-24 23:47 +0000
  ╷  some commit
  ╷
  ╷ o  616bf5  Test User 2018-07-24 05:41 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  d12ab8  Test User 2018-07-24 04:40 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  3d61c7  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  4b67b7  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  56436b  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  06b916  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  d965af  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  665b9e  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  61e66e  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  99509b  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  2cb52d  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  46590a  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  797790  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  ec9300  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  545c51  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  639f1b  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  f50cdb  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  43a3ea  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  083ccd  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b020b9  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  95453e  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  acdbe0  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  206df5  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  758c97  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  fadcc6  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  f50dc4  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b4b391  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  e62712  Test User 2018-07-24 04:36 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  1fe6c7  Test User 2018-07-24 04:36 +0000
  ╭─╯  some commit
  │
  o  445c23 (public)  2018-07-24 04:34 +0000
  ╷  some commit
  ╷
  ╷ o  9ae2eb  Test User 2018-07-23 20:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  75b546  Test User 2018-07-23 20:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  eda0bc  Test User 2018-07-23 20:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  65b8e6  Test User 2018-07-23 20:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b56685  Test User 2018-07-23 20:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  89095b  Test User 2018-07-23 20:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  f9d07a  Test User 2018-07-23 20:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  17afae  Test User 2018-07-23 20:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  eb200c  Test User 2018-07-23 20:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  f5cfc1  Test User 2018-07-23 20:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  bb57a8  Test User 2018-07-23 20:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b01f75  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  485b19  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  86b66e  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  6792bf  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  ca31bb  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  4ccaa1  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  bcf951  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  9d121d  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  cf25e1  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  953880  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b6ac8e  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  7c3fe9  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  602227  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  3ac205  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  cfb3d2  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  50f177  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  9c13c1  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  ee3bd8  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  d911df  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  dce5e2  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  244df6  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b1204f  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  f07c0c  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  183178  Test User 2018-07-23 19:45 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  f6d061  Test User 2018-07-23 19:43 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  932176  Test User 2018-07-23 19:43 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  dfbb04  Test User 2018-07-23 19:43 +0000
  ╭─╯  some commit
  │
  o  55a0d6 (public)  2018-07-23 18:37 +0000
  ╷  some commit
  ╷
  ╷ o  5ff211  Test User 2018-07-19 00:25 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  192d92  Test User 2018-07-19 00:17 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  f36920  Test User 2018-07-19 00:17 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  78503f  Test User 2018-07-19 00:17 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  3a6604  Test User 2018-07-19 00:04 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b593fd  Test User 2018-07-18 23:56 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  65cc49  Test User 2018-07-18 23:46 +0000
  ╭─╯  some commit
  │
  o  90d8c9 (public)  2018-07-18 23:44 +0000
  ╷  some commit
  ╷
  ╷ o  2582d5  Test User 2018-07-03 18:27 +0000
  ╭─╯  some commit
  │
  o  5dd5b0 (public)  2018-07-03 18:26 +0000
  ╷  some commit
  ╷
  ╷ o  fda58c  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  5bba72  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  68315b  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b20853  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  1f4c3f  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  366626  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  11492a  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  98994b  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  dc22fb  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  41b13a  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  1f074c  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  71b2a7  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  ebab1a  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  2e32be  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  18611b  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  05b701  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  3a54e4  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  094e36  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  ec877f  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  c012a1  Test User 2018-06-28 21:55 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  5ef32b  Test User 2018-06-28 21:55 +0000
  ╭─╯  some commit
  │
  o  89c451 (public)  2018-06-28 21:54 +0000
  ╷  some commit
  ╷
  ╷ o  d7de7d  Test User 2018-06-27 21:30 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  a310c0  Test User 2018-06-27 21:30 +0000
  ╭─╯  some commit
  │
  o  944f52 (public)  2018-06-27 21:05 +0000
  ╷  some commit
  ╷
  ╷ o  39dbb5  Test User 2018-06-28 00:03 +0000
  ╭─╯  some commit
  │
  │ o  20917a  Test User 2018-06-27 23:47 +0000
  ├─╯  some commit
  │
  o  c64f72 (public)  2018-06-26 20:45 +0000
  ╷  some commit
  ╷
  ╷ o  d1f01a  Test User 2018-06-27 22:27 +0000
  ╭─╯  some commit
  │
  o  390b78 (public)  2018-06-26 16:53 +0000
  │  some commit
  │
  │ o  b71712  Test User 2018-06-27 22:20 +0000
  ├─╯  some commit
  │
  o  0c157b (public)  2018-06-26 16:53 +0000
  ╷  some commit
  ╷
  ╷ o  d1de39  Test User 2018-06-25 19:12 +0000
  ╭─╯  some commit
  │
  o  ae84d5 (public)  2018-06-25 17:51 +0000
     some commit

  $ cat > $TESTTMP/usersmartlogdata << EOF
  > {
  >   "smartlog": {
  >     "nodes": [
  >       { "node": "01c34a71bd50dafc159a1c55c4c41fd9134ebf4c", "phase": "draft", "author": "Test User", "date": 1522346352, "message": "some commit", "parents": ["346d068bb6c49f594c70a422979ff77bd030a6c2"], "bookmarks": [] },
  >       { "node": "026cdd4c73e7e5886a3c326424dc4ae4c7a7e56d", "phase": "draft", "author": "Test User", "date": 1530004990, "message": "some commit", "parents": ["1bae8ca1d8e15d122badbf3341f4c6bb2f7b2bbc"], "bookmarks": ["somebookmark"] },
  >       { "node": "028179843fe0643bf185286853d33512721bff10", "phase": "public", "author": "Test User", "date": 1530268223, "message": "some commit", "parents": ["74bb207be46da3e778f39820a7acc1cf085bf9d6"], "bookmarks": [] },
  >       { "node": "0319f6ea25d6336a8fdfa789f34bfa396f84ef81", "phase": "draft", "author": "Test User", "date": 1530778679, "message": "some commit", "parents": ["dde91fdd6b0fba8177ea2137d9e039e163e9d04e"], "bookmarks": [] },
  >       { "node": "06dafa1a31ba9e26ffd97a093db6fe40ed3c40ba", "phase": "public", "author": "Test User", "date": 1532514812, "message": "some commit", "parents": ["1e2658a22577c1efc764c08c617831e0d6fd7eab"], "bookmarks": [] },
  >       { "node": "0ccf1aab66281c3018bc02e57885da53457ea934", "phase": "draft", "author": "Test User", "date": 1530268411, "message": "some commit", "parents": ["11a73aa52d175c865143081d914f234a73fd3ea4"], "bookmarks": [] },
  >       { "node": "0e4f249feca6a49ae573a48ea3ef867950ed8b3f", "phase": "public", "author": "Test User", "date": 1530701263, "message": "some commit", "parents": ["15f24fa4f4367e51b9efdaa280d65c96e38ae867"], "bookmarks": [] },
  >       { "node": "113c09ddb5d39109e50cbe0aa56e2cf969a582e7", "phase": "public", "author": "Test User", "date": 1527774422, "message": "some commit", "parents": ["d2f472f6480afb0bdb4b209d6d25ab3b8b756d46"], "bookmarks": [] },
  >       { "node": "113c7f7bddfefb01c179068fce49679507cc1e17", "phase": "draft", "author": "Test User", "date": 1532514599, "message": "some commit", "parents": ["6278d2c06ffd5f95eae9c592d98ffada4760504e"], "bookmarks": [] },
  >       { "node": "11a73aa52d175c865143081d914f234a73fd3ea4", "phase": "draft", "author": "Test User", "date": 1530268411, "message": "some commit", "parents": ["2c008f444ecf468e20cdabf95922449a4d05248a"], "bookmarks": [] },
  >       { "node": "11b378116d286fb9b0acf8e9ec5795fafc47284a", "phase": "draft", "author": "Test User", "date": 1522346359, "message": "some commit", "parents": ["01c34a71bd50dafc159a1c55c4c41fd9134ebf4c"], "bookmarks": [] },
  >       { "node": "17ab2f1765971fadb418499b5061621232fb6422", "phase": "draft", "author": "Test User", "date": 1532522226, "message": "some commit", "parents": ["d464a31f370c1e539edc4a17cb4f30b1b3fd63e6"], "bookmarks": [] },
  >       { "node": "1bae8ca1d8e15d122badbf3341f4c6bb2f7b2bbc", "phase": "draft", "author": "Test User", "date": 1530004989, "message": "some commit", "parents": ["8dc9e32ce16739dc1ed0ab08375137d0e7b9892d"], "bookmarks": [] },
  >       { "node": "1cbc292340f54d4288665abce3d37d9d1af2c253", "phase": "draft", "author": "Test User", "date": 1532514603, "message": "some commit", "parents": ["b2cafefdf4c3244b7612bca24b214d621310fc57"], "bookmarks": [] },
  >       { "node": "20a45149fbc33ccbe5bf34622e0cfcc4c406605c", "phase": "public", "author": "Test User", "date": 1528124965, "message": "some commit", "parents": ["2a76eae50276195a762db390e4dbb0a17bcdeb36"], "bookmarks": [] },
  >       { "node": "21f3bf023340b43fbaf7bc22a1d7eb6a31bc89ec", "phase": "draft", "author": "Test User", "date": 1527777715, "message": "some commit", "parents": ["113c09ddb5d39109e50cbe0aa56e2cf969a582e7"], "bookmarks": [] },
  >       { "node": "234abf34c23d62eda8e9e710e633c623332a7cf0", "phase": "draft", "author": "Test User", "date": 1532522199, "message": "some commit", "parents": ["e966ea02dd871e21d6d8d331587384ede50f510a"], "bookmarks": [] },
  >       { "node": "28d132425377b04334974fee9cb6ffe29393ebea", "phase": "public", "author": "Test User", "date": 1525073350, "message": "some commit", "parents": ["f84e70f0be44f01b704aa3bfa977884c353387f6"], "bookmarks": [] },
  >       { "node": "2c008f444ecf468e20cdabf95922449a4d05248a", "phase": "draft", "author": "Test User", "date": 1530268411, "message": "some commit", "parents": ["028179843fe0643bf185286853d33512721bff10"], "bookmarks": [] },
  >       { "node": "2d5f796b0b9d7ce2f3ee20a00d74e4e82b30ffab", "phase": "draft", "author": "Test User", "date": 1532514596, "message": "some commit", "parents": ["8d0bb391690977e48f0ae90c223f44f84548e58d"], "bookmarks": [] },
  >       { "node": "2e2f42f0861f4f2638bbe6b3f46f6c2f9260912a", "phase": "public", "author": "Test User", "date": 1528788016, "message": "some commit", "parents": ["e83919918ad398b35c76d24ad3bc31ca19fd600e"], "bookmarks": [] },
  >       { "node": "32fff7403eb63d9c714a72761b511c8e19c8b908", "phase": "public", "author": "Test User", "date": 1525107956, "message": "some commit", "parents": ["5d7a73c8c0c2107fd5b1bed74b6b6b6e2a456a7d"], "bookmarks": [] },
  >       { "node": "331e5c2757626d7a82ee985c20fadca27182bc4e", "phase": "draft", "author": "Test User", "date": 1528301971, "message": "some commit", "parents": ["759459674da6ea9192df1045cc3aa80b152ac1e7"], "bookmarks": [] },
  >       { "node": "33254d3fefcc06a270ad5fefb8fed9bf860d1348", "phase": "public", "author": "Test User", "date": 1530175500, "message": "some commit", "parents": ["adc1a4c93e39d64bc0118207c013ec6bfa35bc37"], "bookmarks": [] },
  >       { "node": "346d068bb6c49f594c70a422979ff77bd030a6c2", "phase": "public", "author": "Test User", "date": 1522342182, "message": "some commit", "parents": ["70644e84fe38e833aef8e9a943592fc9feebaa1f"], "bookmarks": [] },
  >       { "node": "35704d32e01a1bc7d15e29fbfebbae1290f6532b", "phase": "draft", "author": "Test User", "date": 1532514919, "message": "some commit", "parents": ["06dafa1a31ba9e26ffd97a093db6fe40ed3c40ba"], "bookmarks": [] },
  >       { "node": "36b6d950c04325e688dc3e64adec34fa05b72690", "phase": "public", "author": "Test User", "date": 1524944872, "message": "some commit", "parents": ["ed2d3f34053f91f54a9f6c4c87d9bded501cb394"], "bookmarks": [] },
  >       { "node": "3922d347cc06f4fbe35e8270d90beab1d2991d1a", "phase": "draft", "author": "Test User", "date": 1532514600, "message": "some commit", "parents": ["39a0933a17c92547061e43c88e9bed2a21915c8a"], "bookmarks": [] },
  >       { "node": "39a0933a17c92547061e43c88e9bed2a21915c8a", "phase": "draft", "author": "Test User", "date": 1532514600, "message": "some commit", "parents": ["e47fc946ff8ba367c04bc02a1febe183604587dc"], "bookmarks": [] },
  >       { "node": "3a9c0a0f4605aa30b591b57fc1823bc78dff219b", "phase": "draft", "author": "Test User", "date": 1528132412, "message": "some commit", "parents": ["af441889d4d960e3ee70680ebf5e333fb82c307e"], "bookmarks": [] },
  >       { "node": "3fc21be55295e545239930c0f16ceaf2d8e0336d", "phase": "public", "author": "Test User", "date": 1529941311, "message": "some commit", "parents": ["5083a361ee9315c1c821040191fd6a09eb75a810"], "bookmarks": [] },
  >       { "node": "418bb9472cd7b28762983b9e7c40422b8d2a4a70", "phase": "draft", "author": "Test User", "date": 1531898896, "message": "some commit", "parents": ["f89701b175e455990e6b19cae9862fa427ac85ac"], "bookmarks": [] },
  >       { "node": "472662ebc0d2005c42595fcdd46a2faf87678fd3", "phase": "draft", "author": "Test User", "date": 1530268412, "message": "some commit", "parents": ["2c008f444ecf468e20cdabf95922449a4d05248a"], "bookmarks": [] },
  >       { "node": "4cf07ae7fd9a1ab994549f5e77eaed09ef862e5b", "phase": "draft", "author": "Test User", "date": 1532522239, "message": "some commit", "parents": ["d464a31f370c1e539edc4a17cb4f30b1b3fd63e6"], "bookmarks": [] },
  >       { "node": "51b718c6b85117043405f38650baaedebf09ca01", "phase": "public", "author": "Test User", "date": 1531898618, "message": "some commit", "parents": ["dfd9e27cd9b67ebc2cedd61be4c00a3867621f71"], "bookmarks": [] },
  >       { "node": "5c1b7ec12e778f53fa8f9615eb97d2a9d2a395c0", "phase": "draft", "author": "Test User", "date": 1525077332, "message": "some commit", "parents": ["28d132425377b04334974fee9cb6ffe29393ebea"], "bookmarks": [] },
  >       { "node": "5d3b9400efa3192b1da0b3969ef3b2eeb2907f2f", "phase": "draft", "author": "Test User", "date": 1530778679, "message": "some commit", "parents": ["73e1363b9999dbdeb7cadb71351bc2f8d2284a9b"], "bookmarks": [] },
  >       { "node": "6278d2c06ffd5f95eae9c592d98ffada4760504e", "phase": "draft", "author": "Test User", "date": 1532514598, "message": "some commit", "parents": ["63f68aa2cb40cd62040cc97d01b25307cad80a90"], "bookmarks": [] },
  >       { "node": "63f68aa2cb40cd62040cc97d01b25307cad80a90", "phase": "draft", "author": "Test User", "date": 1532514598, "message": "some commit", "parents": ["fd261ae6074aa928fecc3ce52012d9b9e2ce8e92"], "bookmarks": [] },
  >       { "node": "6423e05ab26a8f93f8b6f04d35cda6397e2f0e28", "phase": "draft", "author": "Test User", "date": 1532514919, "message": "some commit", "parents": ["35704d32e01a1bc7d15e29fbfebbae1290f6532b"], "bookmarks": [] },
  >       { "node": "64c6caea693dc7179464306491b2f867dfe36c27", "phase": "draft", "author": "Test User", "date": 1528127114, "message": "some commit", "parents": ["20a45149fbc33ccbe5bf34622e0cfcc4c406605c"], "bookmarks": [] },
  >       { "node": "679da9c0e3b2e406cba9d87ae3ac069c497a024d", "phase": "draft", "author": "Test User", "date": 1532514602, "message": "some commit", "parents": ["ac056fc23ddb0eacdf0d2991788995a029286d08"], "bookmarks": [] },
  >       { "node": "68c8920e132594ac53bd92b78c69f60b8aa1e4fe", "phase": "public", "author": "Test User", "date": 1531810131, "message": "some commit", "parents": ["ac7b7d53b9d6395ac9560ffa57a11ee54d22643b"], "bookmarks": [] },
  >       { "node": "6c3f4eb9340d103e3c039dc06c7848490c62778b", "phase": "draft", "author": "Test User", "date": 1530268939, "message": "some commit", "parents": ["f7296235cf4562f3e7b6585cdbae5cce7520b789"], "bookmarks": [] },
  >       { "node": "73e1363b9999dbdeb7cadb71351bc2f8d2284a9b", "phase": "draft", "author": "Test User", "date": 1530778679, "message": "some commit", "parents": ["0319f6ea25d6336a8fdfa789f34bfa396f84ef81"], "bookmarks": [] },
  >       { "node": "743149c1fa9df6348ed1dce57f6842e95c026520", "phase": "draft", "author": "Test User", "date": 1525004667, "message": "some commit", "parents": ["36b6d950c04325e688dc3e64adec34fa05b72690"], "bookmarks": [] },
  >       { "node": "7544e6931bbbe6a3b59c9bbd65de3552a5506e64", "phase": "draft", "author": "Test User", "date": 1532522226, "message": "some commit", "parents": ["17ab2f1765971fadb418499b5061621232fb6422"], "bookmarks": [] },
  >       { "node": "759459674da6ea9192df1045cc3aa80b152ac1e7", "phase": "draft", "author": "Test User", "date": 1528301970, "message": "some commit", "parents": ["f33d9d94b7547d36bd675ce149fccdb8a1705fa7"], "bookmarks": [] },
  >       { "node": "7c08e4847323e49a0eef645304179686b43585a3", "phase": "draft", "author": "Test User", "date": 1529941499, "message": "some commit", "parents": ["8a30022f0ae7753ce4ef374dc5614042c452bbf2"], "bookmarks": [] },
  >       { "node": "7c2d07e7c962e779e74e3dad487abd6b719494f1", "phase": "draft", "author": "Test User", "date": 1530192333, "message": "some commit", "parents": ["33254d3fefcc06a270ad5fefb8fed9bf860d1348"], "bookmarks": [] },
  >       { "node": "7dc7d777c7f67fec5d7208e202aa9f90371911f9", "phase": "draft", "author": "Test User", "date": 1532514601, "message": "some commit", "parents": ["3922d347cc06f4fbe35e8270d90beab1d2991d1a"], "bookmarks": [] },
  >       { "node": "812695ef44ef113f1b528a80374bd48d53132c85", "phase": "draft", "author": "Test User", "date": 1530714894, "message": "some commit", "parents": ["a4e918369260a1e972c333798150bdc87ce6572b"], "bookmarks": [] },
  >       { "node": "8253080283573de7babbaf7b748f5ca545f23747", "phase": "draft", "author": "Test User", "date": 1532514597, "message": "some commit", "parents": ["2d5f796b0b9d7ce2f3ee20a00d74e4e82b30ffab"], "bookmarks": [] },
  >       { "node": "85930ebb891ecdc5d6dc49d362b7f3301b34e4f2", "phase": "draft", "author": "Test User", "date": 1530004989, "message": "some commit", "parents": ["1bae8ca1d8e15d122badbf3341f4c6bb2f7b2bbc"], "bookmarks": [] },
  >       { "node": "883887d07ea3209d64b186583fc9e1094641cb66", "phase": "draft", "author": "Test User", "date": 1531810298, "message": "some commit", "parents": ["68c8920e132594ac53bd92b78c69f60b8aa1e4fe"], "bookmarks": [] },
  >       { "node": "8a30022f0ae7753ce4ef374dc5614042c452bbf2", "phase": "draft", "author": "Test User", "date": 1529941499, "message": "some commit", "parents": ["3fc21be55295e545239930c0f16ceaf2d8e0336d"], "bookmarks": [] },
  >       { "node": "8d0bb391690977e48f0ae90c223f44f84548e58d", "phase": "public", "author": "Test User", "date": 1532503965, "message": "some commit", "parents": ["ea0cdec0334c925303039d4fbf6c5b8002cc4ab0"], "bookmarks": [] },
  >       { "node": "8dc9e32ce16739dc1ed0ab08375137d0e7b9892d", "phase": "public", "author": "Test User", "date": 1530003786, "message": "some commit", "parents": ["61a17acb9a92c62fe2bbd7ff4c3d42909766add7"], "bookmarks": [] },
  >       { "node": "9be605fefbd0b5fb0d9ea9d15760e03df0f5a409", "phase": "draft", "author": "Test User", "date": 1530004989, "message": "some commit", "parents": ["85930ebb891ecdc5d6dc49d362b7f3301b34e4f2"], "bookmarks": [] },
  >       { "node": "9d86687891b54aa267853ab2582db225e5ec829c", "phase": "draft", "author": "Test User", "date": 1525108928, "message": "some commit", "parents": ["d706d6fa2380abaf60def2fd3d6a9f1f45a1cb7e"], "bookmarks": [] },
  >       { "node": "a4e918369260a1e972c333798150bdc87ce6572b", "phase": "draft", "author": "Test User", "date": 1530714894, "message": "some commit", "parents": ["0e4f249feca6a49ae573a48ea3ef867950ed8b3f"], "bookmarks": [] },
  >       { "node": "a81a0c147c885461a3a84df7c083c8815b3aa2dc", "phase": "draft", "author": "Test User", "date": 1525108928, "message": "some commit", "parents": ["9d86687891b54aa267853ab2582db225e5ec829c"], "bookmarks": [] },
  >       { "node": "a8d0fbce4b678f0a1b45fd66de2502ba2007a2e9", "phase": "draft", "author": "Test User", "date": 1530779592, "message": "some commit", "parents": ["d9495d7ebe919306efee403eff89d1210b9dbb76"], "bookmarks": [] },
  >       { "node": "ac056fc23ddb0eacdf0d2991788995a029286d08", "phase": "draft", "author": "Test User", "date": 1532514601, "message": "some commit", "parents": ["7dc7d777c7f67fec5d7208e202aa9f90371911f9"], "bookmarks": [] },
  >       { "node": "af441889d4d960e3ee70680ebf5e333fb82c307e", "phase": "public", "author": "Test User", "date": 1528130962, "message": "some commit", "parents": ["616bb1cb1eb3301eae547ae58876529f1d017cbe"], "bookmarks": [] },
  >       { "node": "b0dfcd35262798eb400120dd2b30c07541945827", "phase": "draft", "author": "Test User", "date": 1531732290, "message": "some commit", "parents": ["fafa02df22a683bacf41a3aff7aecfd688538e5e"], "bookmarks": [] },
  >       { "node": "b2cafefdf4c3244b7612bca24b214d621310fc57", "phase": "draft", "author": "Test User", "date": 1532514602, "message": "some commit", "parents": ["679da9c0e3b2e406cba9d87ae3ac069c497a024d"], "bookmarks": [] },
  >       { "node": "b6d31fc41268b7d9f257a7a6f27ce4452eff9e8b", "phase": "draft", "author": "Test User", "date": 1530714895, "message": "some commit", "parents": ["812695ef44ef113f1b528a80374bd48d53132c85"], "bookmarks": [] },
  >       { "node": "c5a7b9ede7a392b8d1f1f80782aa34dc1a396581", "phase": "draft", "author": "Test User", "date": 1531813273, "message": "some commit", "parents": ["d239f70f31d0a9176ef9dd5710242102e9172c11"], "bookmarks": [] },
  >       { "node": "cc8cce43b187cc0a1e9fbb7a9e5ff504538f6420", "phase": "public", "author": "Test User", "date": 1530775750, "message": "some commit", "parents": ["42404ae027b5ff7bcd09cce2073698c17bdeecda"], "bookmarks": [] },
  >       { "node": "d239f70f31d0a9176ef9dd5710242102e9172c11", "phase": "draft", "author": "Test User", "date": 1531813273, "message": "some commit", "parents": ["883887d07ea3209d64b186583fc9e1094641cb66"], "bookmarks": [] },
  >       { "node": "d464a31f370c1e539edc4a17cb4f30b1b3fd63e6", "phase": "draft", "author": "Test User", "date": 1532522199, "message": "some commit", "parents": ["234abf34c23d62eda8e9e710e633c623332a7cf0"], "bookmarks": [] },
  >       { "node": "d706d6fa2380abaf60def2fd3d6a9f1f45a1cb7e", "phase": "draft", "author": "Test User", "date": 1525108928, "message": "some commit", "parents": ["32fff7403eb63d9c714a72761b511c8e19c8b908"], "bookmarks": [] },
  >       { "node": "d9495d7ebe919306efee403eff89d1210b9dbb76", "phase": "public", "author": "Test User", "date": 1530778746, "message": "some commit", "parents": ["ef49ebc584069f50b4253389e2784880031c7df8"], "bookmarks": [] },
  >       { "node": "dde91fdd6b0fba8177ea2137d9e039e163e9d04e", "phase": "draft", "author": "Test User", "date": 1530778663, "message": "some commit", "parents": ["cc8cce43b187cc0a1e9fbb7a9e5ff504538f6420"], "bookmarks": [] },
  >       { "node": "e0732c3f2410773a64ce4db022c469176eb7f60f", "phase": "public", "author": "Test User", "date": 1529700275, "message": "some commit", "parents": ["0d881e20236d0dfd39a4a211d759d5ba58e13979"], "bookmarks": [] },
  >       { "node": "e47fc946ff8ba367c04bc02a1febe183604587dc", "phase": "draft", "author": "Test User", "date": 1532514599, "message": "some commit", "parents": ["113c7f7bddfefb01c179068fce49679507cc1e17"], "bookmarks": [] },
  >       { "node": "e54f965d477b09eda81248c68d10a60aa652fe12", "phase": "draft", "author": "Test User", "date": 1529723012, "message": "some commit", "parents": ["ea9638e977cf7ab0baadf46c5e33e35177ebbd78"], "bookmarks": [] },
  >       { "node": "e65cc9332f04e26937e2349faa6ba06f8c318d0e", "phase": "draft", "author": "Test User", "date": 1530283913, "message": "some commit", "parents": ["6c3f4eb9340d103e3c039dc06c7848490c62778b"], "bookmarks": [] },
  >       { "node": "e966ea02dd871e21d6d8d331587384ede50f510a", "phase": "draft", "author": "Test User", "date": 1532522198, "message": "some commit", "parents": ["f50344347926facf1946b0a350ea201532d36ffd"], "bookmarks": [] },
  >       { "node": "ea9638e977cf7ab0baadf46c5e33e35177ebbd78", "phase": "draft", "author": "Test User", "date": 1529701973, "message": "some commit", "parents": ["e0732c3f2410773a64ce4db022c469176eb7f60f"], "bookmarks": [] },
  >       { "node": "f33d9d94b7547d36bd675ce149fccdb8a1705fa7", "phase": "public", "author": "Test User", "date": 1528291020, "message": "some commit", "parents": ["0a6d5043f3e598213a6e80963ebd07a015c05cb8"], "bookmarks": [] },
  >       { "node": "f50344347926facf1946b0a350ea201532d36ffd", "phase": "draft", "author": "Test User", "date": 1532522197, "message": "some commit", "parents": ["6423e05ab26a8f93f8b6f04d35cda6397e2f0e28"], "bookmarks": [] },
  >       { "node": "f7296235cf4562f3e7b6585cdbae5cce7520b789", "phase": "draft", "author": "Test User", "date": 1530268846, "message": "some commit", "parents": ["028179843fe0643bf185286853d33512721bff10"], "bookmarks": [] },
  >       { "node": "f89701b175e455990e6b19cae9862fa427ac85ac", "phase": "draft", "author": "Test User", "date": 1531898896, "message": "some commit", "parents": ["51b718c6b85117043405f38650baaedebf09ca01"], "bookmarks": [] },
  >       { "node": "fafa02df22a683bacf41a3aff7aecfd688538e5e", "phase": "public", "author": "Test User", "date": 1531729504, "message": "some commit", "parents": ["6ea0b9c17f8af3a3173b9a5448515940a7cff7a3"], "bookmarks": [] },
  >       { "node": "fd261ae6074aa928fecc3ce52012d9b9e2ce8e92", "phase": "draft", "author": "Test User", "date": 1532514598, "message": "some commit", "parents": ["8253080283573de7babbaf7b748f5ca545f23747"], "bookmarks": [] },
  >       { "node": "ff913801356ccac6398439c32446f31df97a68c2", "phase": "draft", "author": "Test User", "date": 1528789836, "message": "some commit", "parents": ["2e2f42f0861f4f2638bbe6b3f46f6c2f9260912a"], "bookmarks": [] }]
  >   }
  > }
  > EOF

  $ hg cloud sl
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog:
  
    o  7544e6  Test User 2018-07-25 12:37 +0000
    │  some commit
    │
    o  17ab2f  Test User 2018-07-25 12:37 +0000
    │  some commit
    │
    │ o  4cf07a  Test User 2018-07-25 12:37 +0000
    ├─╯  some commit
    │
    o  d464a3  Test User 2018-07-25 12:36 +0000
    │  some commit
    │
    o  234abf  Test User 2018-07-25 12:36 +0000
    │  some commit
    │
    o  e966ea  Test User 2018-07-25 12:36 +0000
    │  some commit
    │
    o  f50344  Test User 2018-07-25 12:36 +0000
    │  some commit
    │
    o  6423e0  Test User 2018-07-25 10:35 +0000
    │  some commit
    │
    o  35704d  Test User 2018-07-25 10:35 +0000
  ╭─╯  some commit
  │
  o  06dafa (public)  2018-07-25 10:33 +0000
  ╷  some commit
  ╷
  ╷ o  1cbc29  Test User 2018-07-25 10:30 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b2cafe  Test User 2018-07-25 10:30 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  679da9  Test User 2018-07-25 10:30 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  ac056f  Test User 2018-07-25 10:30 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  7dc7d7  Test User 2018-07-25 10:30 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  3922d3  Test User 2018-07-25 10:30 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  39a093  Test User 2018-07-25 10:30 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  e47fc9  Test User 2018-07-25 10:29 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  113c7f  Test User 2018-07-25 10:29 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  6278d2  Test User 2018-07-25 10:29 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  63f68a  Test User 2018-07-25 10:29 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  fd261a  Test User 2018-07-25 10:29 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  825308  Test User 2018-07-25 10:29 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  2d5f79  Test User 2018-07-25 10:29 +0000
  ╭─╯  some commit
  │
  o  8d0bb3 (public)  2018-07-25 07:32 +0000
  ╷  some commit
  ╷
  ╷ o  418bb9  Test User 2018-07-18 07:28 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  f89701  Test User 2018-07-18 07:28 +0000
  ╭─╯  some commit
  │
  o  51b718 (public)  2018-07-18 07:23 +0000
  ╷  some commit
  ╷
  ╷ o  c5a7b9  Test User 2018-07-17 07:41 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  d239f7  Test User 2018-07-17 07:41 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  883887  Test User 2018-07-17 06:51 +0000
  ╭─╯  some commit
  │
  o  68c892 (public)  2018-07-17 06:48 +0000
  ╷  some commit
  ╷
  ╷ o  b0dfcd  Test User 2018-07-16 09:11 +0000
  ╭─╯  some commit
  │
  o  fafa02 (public)  2018-07-16 08:25 +0000
  ╷  some commit
  ╷
  ╷ o  a8d0fb  Test User 2018-07-05 08:33 +0000
  ╭─╯  some commit
  │
  o  d9495d (public)  2018-07-05 08:19 +0000
  ╷  some commit
  ╷
  ╷ o  5d3b94  Test User 2018-07-05 08:17 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  73e136  Test User 2018-07-05 08:17 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  0319f6  Test User 2018-07-05 08:17 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  dde91f  Test User 2018-07-05 08:17 +0000
  ╭─╯  some commit
  │
  o  cc8cce (public)  2018-07-05 07:29 +0000
  ╷  some commit
  ╷
  ╷ o  b6d31f  Test User 2018-07-04 14:34 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  812695  Test User 2018-07-04 14:34 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  a4e918  Test User 2018-07-04 14:34 +0000
  ╭─╯  some commit
  │
  o  0e4f24 (public)  2018-07-04 10:47 +0000
  ╷  some commit
  ╷
  ╷ o  0ccf1a  Test User 2018-06-29 10:33 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  11a73a  Test User 2018-06-29 10:33 +0000
  ╷ │  some commit
  ╷ │
  ╷ │ o  472662  Test User 2018-06-29 10:33 +0000
  ╷ ├─╯  some commit
  ╷ │
  ╷ o  2c008f  Test User 2018-06-29 10:33 +0000
  ╭─╯  some commit
  │
  │ o  e65cc9  Test User 2018-06-29 14:51 +0000
  │ │  some commit
  │ │
  │ o  6c3f4e  Test User 2018-06-29 10:42 +0000
  │ │  some commit
  │ │
  │ o  f72962  Test User 2018-06-29 10:40 +0000
  ├─╯  some commit
  │
  o  028179 (public)  2018-06-29 10:30 +0000
  ╷  some commit
  ╷
  ╷ o  7c2d07  Test User 2018-06-28 13:25 +0000
  ╭─╯  some commit
  │
  o  33254d (public)  2018-06-28 08:45 +0000
  ╷  some commit
  ╷
  ╷ o  9be605  Test User 2018-06-26 09:23 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  85930e  Test User 2018-06-26 09:23 +0000
  ╷ │  some commit
  ╷ │
  ╷ │ o  026cdd  Test User 2018-06-26 09:23 +0000 somebookmark
  ╷ ├─╯  some commit
  ╷ │
  ╷ o  1bae8c  Test User 2018-06-26 09:23 +0000
  ╭─╯  some commit
  │
  o  8dc9e3 (public)  2018-06-26 09:03 +0000
  ╷  some commit
  ╷
  ╷ o  7c08e4  Test User 2018-06-25 15:44 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  8a3002  Test User 2018-06-25 15:44 +0000
  ╭─╯  some commit
  │
  o  3fc21b (public)  2018-06-25 15:41 +0000
  ╷  some commit
  ╷
  ╷ o  e54f96  Test User 2018-06-23 03:03 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  ea9638  Test User 2018-06-22 21:12 +0000
  ╭─╯  some commit
  │
  o  e0732c (public)  2018-06-22 20:44 +0000
  ╷  some commit
  ╷
  ╷ o  ff9138  Test User 2018-06-12 07:50 +0000
  ╭─╯  some commit
  │
  o  2e2f42 (public)  2018-06-12 07:20 +0000
  ╷  some commit
  ╷
  ╷ o  331e5c  Test User 2018-06-06 16:19 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  759459  Test User 2018-06-06 16:19 +0000
  ╭─╯  some commit
  │
  o  f33d9d (public)  2018-06-06 13:17 +0000
  ╷  some commit
  ╷
  ╷ o  3a9c0a  Test User 2018-06-04 17:13 +0000
  ╭─╯  some commit
  │
  o  af4418 (public)  2018-06-04 16:49 +0000
  ╷  some commit
  ╷
  ╷ o  64c6ca  Test User 2018-06-04 15:45 +0000
  ╭─╯  some commit
  │
  o  20a451 (public)  2018-06-04 15:09 +0000
  ╷  some commit
  ╷
  ╷ o  21f3bf  Test User 2018-05-31 14:41 +0000
  ╭─╯  some commit
  │
  o  113c09 (public)  2018-05-31 13:47 +0000
  ╷  some commit
  ╷
  ╷ o  a81a0c  Test User 2018-04-30 17:22 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  9d8668  Test User 2018-04-30 17:22 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  d706d6  Test User 2018-04-30 17:22 +0000
  ╭─╯  some commit
  │
  o  32fff7 (public)  2018-04-30 17:05 +0000
  ╷  some commit
  ╷
  ╷ o  5c1b7e  Test User 2018-04-30 08:35 +0000
  ╭─╯  some commit
  │
  o  28d132 (public)  2018-04-30 07:29 +0000
  ╷  some commit
  ╷
  ╷ o  743149  Test User 2018-04-29 12:24 +0000
  ╭─╯  some commit
  │
  o  36b6d9 (public)  2018-04-28 19:47 +0000
  ╷  some commit
  ╷
  ╷ o  11b378  Test User 2018-03-29 17:59 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  01c34a  Test User 2018-03-29 17:59 +0000
  ╭─╯  some commit
  │
  o  346d06 (public)  2018-03-29 16:49 +0000
     some commit

  $ cat > $TESTTMP/usersmartlogdata << EOF
  > {
  >   "smartlog": {
  >     "nodes": [
  >       { "node": "009060be2a986cd8400700a790fa37017d296c8c", "phase": "public", "author": "Test User", "date": 1505360696, "message": "some commit", "parents": ["1a0636bdc034bfd0114de85f1fde08a2ab11d98e"], "bookmarks": [] },
  >       { "node": "01c34a71bd50dafc159a1c55c4c41fd9134ebf4c", "phase": "draft", "author": "Test User", "date": 1522346352, "message": "some commit", "parents": ["346d068bb6c49f594c70a422979ff77bd030a6c2"], "bookmarks": [] },
  >       { "node": "03012730f28cb61aa5f17476948140713ef81cb1", "phase": "public", "author": "Test User", "date": 1493421697, "message": "some commit", "parents": ["1da689adbce931789a54abb5178f0a86a66a3b81"], "bookmarks": [] },
  >       { "node": "04d77081e5bd992e046e81ceba0292032222e508", "phase": "draft", "author": "Test User", "date": 1528698393, "message": "some commit", "parents": ["dce9d21a58c5d5d353145b6fe24c0da4b6c9a94c"], "bookmarks": [] },
  >       { "node": "0e7cbef2099cdb2d0141fa93a1364cd77aa79b76", "phase": "draft", "author": "Test User", "date": 1529355239, "message": "some commit", "parents": ["b7f63b7fe3a46884448426a561bfdb9247b226c1"], "bookmarks": [] },
  >       { "node": "0ed0d8de85d497c81610d4b8cfd47a8258649f5c", "phase": "public", "author": "Test User", "date": 1504612169, "message": "some commit", "parents": ["1ea9fa3b165edd1ee5143a6d55f393129e851a19"], "bookmarks": [] },
  >       { "node": "10f94cd91e53d7b243a1959d42592a7a7b327aed", "phase": "draft", "author": "Test User", "date": 1493422322, "message": "some commit", "parents": ["708ef018de0db84f1bbab9dc1aac3c49ad43ea7d"], "bookmarks": [] },
  >       { "node": "11b378116d286fb9b0acf8e9ec5795fafc47284a", "phase": "draft", "author": "Test User", "date": 1522346359, "message": "some commit", "parents": ["01c34a71bd50dafc159a1c55c4c41fd9134ebf4c"], "bookmarks": [] },
  >       { "node": "11d37211d6cf87824af891458dad6dc5546c8a4f", "phase": "draft", "author": "Test User", "date": 1521716864, "message": "some commit", "parents": ["d0fc06a6bca08e8fad66297e9cd7e61e00ebee2b"], "bookmarks": [] },
  >       { "node": "133633f1919a66c6507de96f2c62e945545c80bc", "phase": "draft", "author": "Test User", "date": 1521717816, "message": "some commit", "parents": ["11d37211d6cf87824af891458dad6dc5546c8a4f"], "bookmarks": [] },
  >       { "node": "1424fac209be9e5c5d45dfbfc11132185a6f140b", "phase": "draft", "author": "Test User", "date": 1532014317, "message": "some commit", "parents": ["91514fdf2e6cb7766bbaf97ce696f87102e6e38a"], "bookmarks": [] },
  >       { "node": "17c3ccaec9f5a57db696850d9819ba7308616ffa", "phase": "draft", "author": "Test User", "date": 1529355233, "message": "some commit", "parents": ["79e25172c4631c42217fdf45680eba0fd4b62345"], "bookmarks": [] },
  >       { "node": "194b51c08865b03f37338e1305cf6d8e4e5b9a9d", "phase": "draft", "author": "Test User", "date": 1529355208, "message": "some commit", "parents": ["c2c53d71939f35bce0f1a29769f21d33f9ad867d"], "bookmarks": [] },
  >       { "node": "1b7f2fbb364a14262083b78926542247e584c6d1", "phase": "public", "author": "Test User", "date": 1527282392, "message": "some commit", "parents": ["61aa06e1466374a168fc0d5bc3b6bdec268caa4c"], "bookmarks": [] },
  >       { "node": "2711a39a5864b77e6772e230d60612d5e33a9cbf", "phase": "draft", "author": "Test User", "date": 1532465598, "message": "some commit", "parents": ["3bfb42cab7ab78485ce3f98532e7331136ad0b22"], "bookmarks": [] },
  >       { "node": "2c378cc24b176ee9bd7d22ac4ac3e2ca84cea4ad", "phase": "public", "author": "Test User", "date": 1506536794, "message": "some commit", "parents": ["bdfa68bc50540b6db71158658c621039821dd0bf"], "bookmarks": [] },
  >       { "node": "305f0f2285ea2d22d21f3499f0308f551de9ad76", "phase": "public", "author": "Test User", "date": 1525811963, "message": "some commit", "parents": ["b6822e78bdb840206e6994ecaced2ef6cd9f86fd"], "bookmarks": [] },
  >       { "node": "3076b0c1368886c2532e91ee0a4e07a2deff5c08", "phase": "public", "author": "Test User", "date": 1525707227, "message": "some commit", "parents": ["509276a4a01a96b7be4ff023621b53f4be087213"], "bookmarks": [] },
  >       { "node": "31cac058b7ac9868ab45a248e8c87066695b2e96", "phase": "draft", "author": "Test User", "date": 1506551069, "message": "some commit", "parents": ["2c378cc24b176ee9bd7d22ac4ac3e2ca84cea4ad"], "bookmarks": [] },
  >       { "node": "3395c3481cc52df943ba36590f57df962efce66c", "phase": "draft", "author": "Test User", "date": 1529441644, "message": "some commit", "parents": ["9734e2e1a357b41041a8fcc4a990f0ace2463e66"], "bookmarks": [] },
  >       { "node": "33e8c2ca21065fd84e51d3eb66c4c54def245423", "phase": "public", "author": "Test User", "date": 1532368989, "message": "some commit", "parents": ["dc1f4c1fa3ccbbbae98e802137092fbda51da7b2"], "bookmarks": [] },
  >       { "node": "346d068bb6c49f594c70a422979ff77bd030a6c2", "phase": "public", "author": "Test User", "date": 1522342182, "message": "some commit", "parents": ["70644e84fe38e833aef8e9a943592fc9feebaa1f"], "bookmarks": [] },
  >       { "node": "3892486f3655ec6b4f55df00e9e6de7191a097fe", "phase": "draft", "author": "Test User", "date": 1527283646, "message": "some commit", "parents": ["1b7f2fbb364a14262083b78926542247e584c6d1"], "bookmarks": [] },
  >       { "node": "38b901ebaed2b90ec6c084175f7e50c28b588116", "phase": "draft", "author": "Test User", "date": 1504536364, "message": "some commit", "parents": ["428ae8cbfcfc5d48c17cb9e0e86fceee14b3b1eb"], "bookmarks": [] },
  >       { "node": "3bfb42cab7ab78485ce3f98532e7331136ad0b22", "phase": "public", "author": "Test User", "date": 1532445569, "message": "some commit", "parents": ["b0473142edd1e68eccc620898a75d3a5117146ab"], "bookmarks": [] },
  >       { "node": "3e75a922445d682571e606f2ed54ade602ec4ab9", "phase": "draft", "author": "Test User", "date": 1525710578, "message": "some commit", "parents": ["3076b0c1368886c2532e91ee0a4e07a2deff5c08"], "bookmarks": [] },
  >       { "node": "3f4905d7911abc882095f26fc05f99ae56e42f64", "phase": "draft", "author": "Test User", "date": 1510173468, "message": "some commit", "parents": ["a9a8d86cfbfe647c00c64dccab47ed3e401cd6a6"], "bookmarks": [] },
  >       { "node": "428ae8cbfcfc5d48c17cb9e0e86fceee14b3b1eb", "phase": "public", "author": "Test User", "date": 1504527018, "message": "some commit", "parents": ["639d02d732eb228707f2bc8cc7256ddf1a2d3c61"], "bookmarks": [] },
  >       { "node": "54b9cf05de32e2303ebc508fa4867eef3c99cb0b", "phase": "draft", "author": "Test User", "date": 1509754259, "message": "some commit", "parents": ["9af6a3ee205b38a5e1fbd9a9b0d215e6a8b06582"], "bookmarks": [] },
  >       { "node": "5bbb0508b290253a4d9e91a90c5f1aadec214ddb", "phase": "draft", "author": "Test User", "date": 1521717816, "message": "some commit", "parents": ["e62dcaa05c8e5e23b209e2ac6230b20ae67dd741"], "bookmarks": [] },
  >       { "node": "5c8bce3a3b1e3b6a5ca445e9a3d4c8c134e5f403", "phase": "draft", "author": "Test User", "date": 1505360909, "message": "some commit", "parents": ["009060be2a986cd8400700a790fa37017d296c8c"], "bookmarks": [] },
  >       { "node": "6403af302ce7d2f931183c1ffe4234b681c9f33d", "phase": "draft", "author": "Test User", "date": 1522346359, "message": "some commit", "parents": ["11b378116d286fb9b0acf8e9ec5795fafc47284a"], "bookmarks": [] },
  >       { "node": "708ef018de0db84f1bbab9dc1aac3c49ad43ea7d", "phase": "draft", "author": "Test User", "date": 1493422322, "message": "some commit", "parents": ["a741899332bf6be2bb8dd060eda86a4578ed93dc"], "bookmarks": [] },
  >       { "node": "737b4bc673425949df9bf71c11d6ddde9e73d4b7", "phase": "draft", "author": "Test User", "date": 1529446806, "message": "some commit", "parents": ["f7b43d89336bb1fe02a03403d5f7b5468c34512f"], "bookmarks": [] },
  >       { "node": "77d290179eed304f1bacf8d401c190c47dc05104", "phase": "draft", "author": "Test User", "date": 1529355239, "message": "some commit", "parents": ["17c3ccaec9f5a57db696850d9819ba7308616ffa"], "bookmarks": [] },
  >       { "node": "78889e83ed57d155a48d1860e6a9539246b83068", "phase": "draft", "author": "Test User", "date": 1504614082, "message": "some commit", "parents": ["0ed0d8de85d497c81610d4b8cfd47a8258649f5c"], "bookmarks": [] },
  >       { "node": "79e25172c4631c42217fdf45680eba0fd4b62345", "phase": "draft", "author": "Test User", "date": 1529355208, "message": "some commit", "parents": ["b7998bcfdae80da8f83eb6ae155188d6523e6c1e"], "bookmarks": [] },
  >       { "node": "7ea151b7e17414475142f15f2fdc91e10ca1786d", "phase": "draft", "author": "Test User", "date": 1516212505, "message": "some commit", "parents": ["b0e7aebd071a54bdb215ba94df8e0a92920c8ffb"], "bookmarks": [] },
  >       { "node": "818195d3da5e7b56cf79b27ab2e16fd96c9c19e0", "phase": "draft", "author": "Test User", "date": 1493422322, "message": "some commit", "parents": ["03012730f28cb61aa5f17476948140713ef81cb1"], "bookmarks": [] },
  >       { "node": "8acf52d068d3bb17763cc893036a0f233a4d353a", "phase": "draft", "author": "Test User", "date": 1532465609, "message": "some commit", "parents": ["3bfb42cab7ab78485ce3f98532e7331136ad0b22"], "bookmarks": [] },
  >       { "node": "8fc9f7903f3892b547bfe73452e988a204682acb", "phase": "public", "author": "Test User", "date": 1527528142, "message": "some commit", "parents": ["b0785e6c130fba6021e63fb027ccd203d946dde9"], "bookmarks": [] },
  >       { "node": "91514fdf2e6cb7766bbaf97ce696f87102e6e38a", "phase": "public", "author": "Test User", "date": 1532013238, "message": "some commit", "parents": ["aa56eb46b2cee096c0b6197342c271f7c9e99c26"], "bookmarks": [] },
  >       { "node": "9734e2e1a357b41041a8fcc4a990f0ace2463e66", "phase": "public", "author": "Test User", "date": 1529354025, "message": "some commit", "parents": ["c4cf8f0c4334c1a91d4b1cbb1ae1c9242375d66e"], "bookmarks": [] },
  >       { "node": "9a3e9e13ca151698e87a421dff4b5584cb1b7137", "phase": "draft", "author": "Test User", "date": 1529443531, "message": "some commit", "parents": ["f7b43d89336bb1fe02a03403d5f7b5468c34512f"], "bookmarks": [] },
  >       { "node": "9af6a3ee205b38a5e1fbd9a9b0d215e6a8b06582", "phase": "public", "author": "Test User", "date": 1509744595, "message": "some commit", "parents": ["aacc5873ecc49c96c2346dde386239d0f8ba9e60"], "bookmarks": [] },
  >       { "node": "a55ab5cc28723bd2cd3cde7da276bc4f09d11d9a", "phase": "public", "author": "Test User", "date": 1519757138, "message": "some commit", "parents": ["2b46ccd0b0d3a6208d804d805202f46f64f9f22f"], "bookmarks": [] },
  >       { "node": "a741899332bf6be2bb8dd060eda86a4578ed93dc", "phase": "draft", "author": "Test User", "date": 1493422322, "message": "some commit", "parents": ["818195d3da5e7b56cf79b27ab2e16fd96c9c19e0"], "bookmarks": [] },
  >       { "node": "a9a8d86cfbfe647c00c64dccab47ed3e401cd6a6", "phase": "public", "author": "Test User", "date": 1510173415, "message": "some commit", "parents": ["0ea903026dddfe488c2af8361321c0cb63907d63"], "bookmarks": [] },
  >       { "node": "ae05a99634f038228c94bf4dbb354242392d80b3", "phase": "public", "author": "Test User", "date": 1516212425, "message": "some commit", "parents": ["7114f3f3e6bbc08dfe20075e32fa33926735034a"], "bookmarks": [] },
  >       { "node": "af18959e19cd1f354a5372a4bd1f553182e7e4fb", "phase": "draft", "author": "Test User", "date": 1520443643, "message": "some commit", "parents": ["bc36d3292fc711f12ca72a816060a5e082974bf2"], "bookmarks": [] },
  >       { "node": "b0e7aebd071a54bdb215ba94df8e0a92920c8ffb", "phase": "draft", "author": "Test User", "date": 1516212505, "message": "some commit", "parents": ["ae05a99634f038228c94bf4dbb354242392d80b3"], "bookmarks": [] },
  >       { "node": "b1663b916c0a8d61fd9ce177266be81400d9260b", "phase": "draft", "author": "Test User", "date": 1532370214, "message": "some commit", "parents": ["33e8c2ca21065fd84e51d3eb66c4c54def245423"], "bookmarks": [] },
  >       { "node": "b7998bcfdae80da8f83eb6ae155188d6523e6c1e", "phase": "draft", "author": "Test User", "date": 1529355208, "message": "some commit", "parents": ["194b51c08865b03f37338e1305cf6d8e4e5b9a9d"], "bookmarks": [] },
  >       { "node": "b7f63b7fe3a46884448426a561bfdb9247b226c1", "phase": "draft", "author": "Test User", "date": 1529355239, "message": "some commit", "parents": ["77d290179eed304f1bacf8d401c190c47dc05104"], "bookmarks": [] },
  >       { "node": "b9f2b3b3bbc307daa7b4e8d75ba3d9c0ce97f7a5", "phase": "public", "author": "Test User", "date": 1503345256, "message": "some commit", "parents": ["bec9051a78e480b21df2c0868ee1ab94ba337fe7"], "bookmarks": [] },
  >       { "node": "bc36d3292fc711f12ca72a816060a5e082974bf2", "phase": "public", "author": "Test User", "date": 1520290399, "message": "some commit", "parents": ["bc5d14abdf6bf8cdf8ff6a352b3b2a24f8210add"], "bookmarks": [] },
  >       { "node": "c2c53d71939f35bce0f1a29769f21d33f9ad867d", "phase": "draft", "author": "Test User", "date": 1529355208, "message": "some commit", "parents": ["3395c3481cc52df943ba36590f57df962efce66c"], "bookmarks": [] },
  >       { "node": "cba7a68b19634e64840dc1d2d8209d83385aa3ee", "phase": "draft", "author": "Test User", "date": 1519758726, "message": "some commit", "parents": ["a55ab5cc28723bd2cd3cde7da276bc4f09d11d9a"], "bookmarks": [] },
  >       { "node": "cbc789314a0b8133b896f02a2f634c6048232041", "phase": "draft", "author": "Test User", "date": 1527612412, "message": "some commit", "parents": ["8fc9f7903f3892b547bfe73452e988a204682acb"], "bookmarks": [] },
  >       { "node": "d0fc06a6bca08e8fad66297e9cd7e61e00ebee2b", "phase": "public", "author": "Test User", "date": 1521716414, "message": "some commit", "parents": ["2f7844f19a6448866a161969e065584437e211d3"], "bookmarks": [] },
  >       { "node": "d2f740307241af82c25a4eac4c6737235ca7a208", "phase": "draft", "author": "Test User", "date": 1532014317, "message": "some commit", "parents": ["1424fac209be9e5c5d45dfbfc11132185a6f140b"], "bookmarks": [] },
  >       { "node": "d5f813490d02d34b714efc841881f689814a79eb", "phase": "draft", "author": "Test User", "date": 1504614082, "message": "some commit", "parents": ["78889e83ed57d155a48d1860e6a9539246b83068"], "bookmarks": [] },
  >       { "node": "dbc73da9dd70f4503828eed3c58d49bd922e8b44", "phase": "public", "author": "Test User", "date": 1492123992, "message": "some commit", "parents": ["a5e01e3dc233cc14fc7fcf7730557d266a97875f"], "bookmarks": [] },
  >       { "node": "dce9d21a58c5d5d353145b6fe24c0da4b6c9a94c", "phase": "public", "author": "Test User", "date": 1528616235, "message": "some commit", "parents": ["d6c7ed4c0859b5dcf1cde2e3e0a93c71dec4893a"], "bookmarks": [] },
  >       { "node": "debd4d483c7adb9fb701c06606a9f6630996ee5a", "phase": "draft", "author": "Test User", "date": 1525848690, "message": "some commit", "parents": ["305f0f2285ea2d22d21f3499f0308f551de9ad76"], "bookmarks": [] },
  >       { "node": "e5dfd022ff1b2ebcdb4809590754623b3d29da75", "phase": "draft", "author": "Test User", "date": 1521717816, "message": "some commit", "parents": ["133633f1919a66c6507de96f2c62e945545c80bc"], "bookmarks": [] },
  >       { "node": "e62dcaa05c8e5e23b209e2ac6230b20ae67dd741", "phase": "draft", "author": "Test User", "date": 1521717816, "message": "some commit", "parents": ["e5dfd022ff1b2ebcdb4809590754623b3d29da75"], "bookmarks": [] },
  >       { "node": "e9aebcf37573924a4ba6f3c91e1e608b4dc46db3", "phase": "draft", "author": "Test User", "date": 1492292129, "message": "some commit", "parents": ["dbc73da9dd70f4503828eed3c58d49bd922e8b44"], "bookmarks": ["somebookmark"] },
  >       { "node": "f7b43d89336bb1fe02a03403d5f7b5468c34512f", "phase": "public", "author": "Test User", "date": 1529364929, "message": "some commit", "parents": ["1d48afcd6bef1c402e324bc1a1bc63d4f2258c5a"], "bookmarks": [] },
  >       { "node": "fe391c05494f5c1d6801c733253a2bb866a088e6", "phase": "draft", "author": "Test User", "date": 1503347762, "message": "some commit", "parents": ["b9f2b3b3bbc307daa7b4e8d75ba3d9c0ce97f7a5"], "bookmarks": [] }]
  >   }
  > }
  > EOF

  $ hg cloud sl
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog:
  
    o  8acf52  Test User 2018-07-24 20:53 +0000
  ╭─╯  some commit
  │
  │ o  2711a3  Test User 2018-07-24 20:53 +0000
  ├─╯  some commit
  │
  o  3bfb42 (public)  2018-07-24 15:19 +0000
  ╷  some commit
  ╷
  ╷ o  b1663b  Test User 2018-07-23 18:23 +0000
  ╭─╯  some commit
  │
  o  33e8c2 (public)  2018-07-23 18:03 +0000
  ╷  some commit
  ╷
  ╷ o  d2f740  Test User 2018-07-19 15:31 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  1424fa  Test User 2018-07-19 15:31 +0000
  ╭─╯  some commit
  │
  o  91514f (public)  2018-07-19 15:13 +0000
  ╷  some commit
  ╷
  ╷ o  9a3e9e  Test User 2018-06-19 21:25 +0000
  ╭─╯  some commit
  │
  │ o  737b4b  Test User 2018-06-19 22:20 +0000
  ├─╯  some commit
  │
  o  f7b43d (public)  2018-06-18 23:35 +0000
  ╷  some commit
  ╷
  ╷ o  0e7cbe  Test User 2018-06-18 20:53 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b7f63b  Test User 2018-06-18 20:53 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  77d290  Test User 2018-06-18 20:53 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  17c3cc  Test User 2018-06-18 20:53 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  79e251  Test User 2018-06-18 20:53 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b7998b  Test User 2018-06-18 20:53 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  194b51  Test User 2018-06-18 20:53 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  c2c53d  Test User 2018-06-18 20:53 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  3395c3  Test User 2018-06-19 20:54 +0000
  ╭─╯  some commit
  │
  o  9734e2 (public)  2018-06-18 20:33 +0000
  ╷  some commit
  ╷
  ╷ o  04d770  Test User 2018-06-11 06:26 +0000
  ╭─╯  some commit
  │
  o  dce9d2 (public)  2018-06-10 07:37 +0000
  ╷  some commit
  ╷
  ╷ o  cbc789  Test User 2018-05-29 16:46 +0000
  ╭─╯  some commit
  │
  o  8fc9f7 (public)  2018-05-28 17:22 +0000
  ╷  some commit
  ╷
  ╷ o  389248  Test User 2018-05-25 21:27 +0000
  ╭─╯  some commit
  │
  o  1b7f2f (public)  2018-05-25 21:06 +0000
  ╷  some commit
  ╷
  ╷ o  debd4d  Test User 2018-05-09 06:51 +0000
  ╭─╯  some commit
  │
  o  305f0f (public)  2018-05-08 20:39 +0000
  ╷  some commit
  ╷
  ╷ o  3e75a9  Test User 2018-05-07 16:29 +0000
  ╭─╯  some commit
  │
  o  3076b0 (public)  2018-05-07 15:33 +0000
  ╷  some commit
  ╷
  ╷ o  6403af  Test User 2018-03-29 17:59 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  11b378  Test User 2018-03-29 17:59 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  01c34a  Test User 2018-03-29 17:59 +0000
  ╭─╯  some commit
  │
  o  346d06 (public)  2018-03-29 16:49 +0000
  ╷  some commit
  ╷
  ╷ o  5bbb05  Test User 2018-03-22 11:23 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  e62dca  Test User 2018-03-22 11:23 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  e5dfd0  Test User 2018-03-22 11:23 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  133633  Test User 2018-03-22 11:23 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  11d372  Test User 2018-03-22 11:07 +0000
  ╭─╯  some commit
  │
  o  d0fc06 (public)  2018-03-22 11:00 +0000
  ╷  some commit
  ╷
  ╷ o  af1895  Test User 2018-03-07 17:27 +0000
  ╭─╯  some commit
  │
  o  bc36d3 (public)  2018-03-05 22:53 +0000
  ╷  some commit
  ╷
  ╷ o  cba7a6  Test User 2018-02-27 19:12 +0000
  ╭─╯  some commit
  │
  o  a55ab5 (public)  2018-02-27 18:45 +0000
  ╷  some commit
  ╷
  ╷ o  7ea151  Test User 2018-01-17 18:08 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  b0e7ae  Test User 2018-01-17 18:08 +0000
  ╭─╯  some commit
  │
  o  ae05a9 (public)  2018-01-17 18:07 +0000
  ╷  some commit
  ╷
  ╷ o  3f4905  Test User 2017-11-08 20:37 +0000
  ╭─╯  some commit
  │
  o  a9a8d8 (public)  2017-11-08 20:36 +0000
  ╷  some commit
  ╷
  ╷ o  54b9cf  Test User 2017-11-04 00:10 +0000
  ╭─╯  some commit
  │
  o  9af6a3 (public)  2017-11-03 21:29 +0000
  ╷  some commit
  ╷
  ╷ o  31cac0  Test User 2017-09-27 22:24 +0000
  ╭─╯  some commit
  │
  o  2c378c (public)  2017-09-27 18:26 +0000
  ╷  some commit
  ╷
  ╷ o  5c8bce  Test User 2017-09-14 03:48 +0000
  ╭─╯  some commit
  │
  o  009060 (public)  2017-09-14 03:44 +0000
  ╷  some commit
  ╷
  ╷ o  d5f813  Test User 2017-09-05 12:21 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  78889e  Test User 2017-09-05 12:21 +0000
  ╭─╯  some commit
  │
  o  0ed0d8 (public)  2017-09-05 11:49 +0000
  ╷  some commit
  ╷
  ╷ o  38b901  Test User 2017-09-04 14:46 +0000
  ╭─╯  some commit
  │
  o  428ae8 (public)  2017-09-04 12:10 +0000
  ╷  some commit
  ╷
  ╷ o  fe391c  Test User 2017-08-21 20:36 +0000
  ╭─╯  some commit
  │
  o  b9f2b3 (public)  2017-08-21 19:54 +0000
  ╷  some commit
  ╷
  ╷ o  10f94c  Test User 2017-04-28 23:32 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  708ef0  Test User 2017-04-28 23:32 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  a74189  Test User 2017-04-28 23:32 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  818195  Test User 2017-04-28 23:32 +0000
  ╭─╯  some commit
  │
  o  030127 (public)  2017-04-28 23:21 +0000
  ╷  some commit
  ╷
  ╷ o  e9aebc  Test User 2017-04-15 21:35 +0000 somebookmark
  ╭─╯  some commit
  │
  o  dbc73d (public)  2017-04-13 22:53 +0000
     some commit

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

  $ hg cloud sl
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog:
  
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

  $ cat > $TESTTMP/usersmartlogdata << EOF
  > {
  >   "smartlog": {
  >     "nodes": [
  >       { "node": "05e8283aac6a0f6823ac46a01d47bf0d78c74352", "phase": "draft", "author": "Test User", "date": 1529248423, "message": "some commit", "parents": ["098db6c5d964b3cfdb7e864b9175e03c7c8dd1df"], "bookmarks": [] },
  >       { "node": "073f9863817170dd25574c86fcb1325422711e21", "phase": "public", "author": "Test User", "date": 1514887213, "message": "some commit", "parents": ["0ad1ccbbd787dab00cf5fcf360e4c888479a399d"], "bookmarks": [] },
  >       { "node": "098db6c5d964b3cfdb7e864b9175e03c7c8dd1df", "phase": "public", "author": "Test User", "date": 1529248364, "message": "some commit", "parents": ["3b13cea0cdfb61df9210660014df141ce3c3de8a"], "bookmarks": [] },
  >       { "node": "0f6762306cb9f73672062b905fa9ab0cfa30395f", "phase": "draft", "author": "Test User", "date": 1514895955, "message": "some commit", "parents": ["ae48faf1844bfc184b7897d27594f251a9b627dc"], "bookmarks": [] },
  >       { "node": "18ead8dcb4016197962986b9a10317ec6b7e5535", "phase": "draft", "author": "Test User", "date": 1529336306, "message": "some commit", "parents": ["343314c6da8bc9d714eed86daaca4a393139fd97"], "bookmarks": [] },
  >       { "node": "192484cd863eeabc3f07520efe24c5530d0abd10", "phase": "draft", "author": "Test User", "date": 1529336306, "message": "some commit", "parents": ["d982aa936917d880a40a250ec1aa4250af062cba"], "bookmarks": [] },
  >       { "node": "22bc0fbef62362d9d7f462c21b1ebccd08a47509", "phase": "draft", "author": "Test User", "date": 1529248423, "message": "some commit", "parents": ["05e8283aac6a0f6823ac46a01d47bf0d78c74352"], "bookmarks": [] },
  >       { "node": "25321d75dd6661cbff80d8c983a84954758ac53f", "phase": "draft", "author": "Test User", "date": 1529248423, "message": "some commit", "parents": ["941218e740b1d012cff9f9ea77adddbc6c224e4c"], "bookmarks": [] },
  >       { "node": "25d91f809f70b93803575fd686df659ad01a1ee4", "phase": "draft", "author": "Test User", "date": 1529248423, "message": "some commit", "parents": ["c4e2cdb652d279c6f4078029d70f6e82028db9ff"], "bookmarks": [] },
  >       { "node": "2ad8adc4e047d6e7b328b5bf4a23f0a5665c2072", "phase": "draft", "author": "Test User", "date": 1529336306, "message": "some commit", "parents": ["a35c2de4bbd0fa50f2de1fd5e80e3fc51a131efa"], "bookmarks": [] },
  >       { "node": "2b9a5259bdf4dbf6bcb8e7388bc16a82827fbca0", "phase": "draft", "author": "Test User", "date": 1529248423, "message": "some commit", "parents": ["5c45cd462c83b61c83123d5a4d36553f5ef9a54c"], "bookmarks": [] },
  >       { "node": "32f304dc8c78f2b0a0a43271e67c861b1f749354", "phase": "draft", "author": "Test User", "date": 1520031322, "message": "some commit", "parents": ["76159c5110bd80de8eeddb82410e3533375c1e52"], "bookmarks": [] },
  >       { "node": "343314c6da8bc9d714eed86daaca4a393139fd97", "phase": "draft", "author": "Test User", "date": 1529336306, "message": "some commit", "parents": ["d50e126ac2a6eda9b5a1121c32bf0bf89de58698"], "bookmarks": [] },
  >       { "node": "41c3c66faeaed2b5771deeb4a7b5fd32e1f80ae5", "phase": "public", "author": "Test User", "date": 1528382502, "message": "some commit", "parents": ["5c6f83c5a8eeb48dfad68bc52cba3b92bc288541"], "bookmarks": [] },
  >       { "node": "43b76c388a8e78d8b08073cc9d0989cde7a0c4dd", "phase": "public", "author": "Test User", "date": 1526915113, "message": "some commit", "parents": ["24165dd32b08822aa8d10028f24cd9a360721388"], "bookmarks": [] },
  >       { "node": "4ac122ed3ca889c9ba0850b5932cca13299367e9", "phase": "draft", "author": "Test User", "date": 1529336306, "message": "some commit", "parents": ["2ad8adc4e047d6e7b328b5bf4a23f0a5665c2072"], "bookmarks": [] },
  >       { "node": "4c1ab442d10467487ecdaa07189e4c93a7501e93", "phase": "public", "author": "Test User", "date": 1528735619, "message": "some commit", "parents": ["47825e06c7205dbe005f209f393f313027a5ea62"], "bookmarks": [] },
  >       { "node": "5061177965f781dcfdb21dc9b46bd53e699a78d8", "phase": "draft", "author": "Test User", "date": 1528901845, "message": "some commit", "parents": ["41c3c66faeaed2b5771deeb4a7b5fd32e1f80ae5"], "bookmarks": [] },
  >       { "node": "5c45cd462c83b61c83123d5a4d36553f5ef9a54c", "phase": "draft", "author": "Test User", "date": 1529248423, "message": "some commit", "parents": ["25d91f809f70b93803575fd686df659ad01a1ee4"], "bookmarks": [] },
  >       { "node": "5c518c325594dc279b3068133a4b41f4562844c7", "phase": "public", "author": "Test User", "date": 1526914663, "message": "some commit", "parents": ["e4329abb9b26a6545bb9979349d8792751e9b4c6"], "bookmarks": [] },
  >       { "node": "5fdfe29f34cd4d45c23ece398adc684343efd46e", "phase": "draft", "author": "Test User", "date": 1529336306, "message": "some commit", "parents": ["7e8e6af6102a2637f4a84d700d1a7616657b9a30"], "bookmarks": [] },
  >       { "node": "6e4d02e68704f8505023edb2a4b4487f2df2e01a", "phase": "draft", "author": "Test User", "date": 1518445327, "message": "some commit", "parents": ["81e4fb4dba036a04d43c6bce369bb0b77b92e0af"], "bookmarks": [] },
  >       { "node": "76159c5110bd80de8eeddb82410e3533375c1e52", "phase": "public", "author": "Test User", "date": 1520030589, "message": "some commit", "parents": ["434148ced185bb0f96d900dee12593d981c400ae"], "bookmarks": [] },
  >       { "node": "7e8e6af6102a2637f4a84d700d1a7616657b9a30", "phase": "draft", "author": "Test User", "date": 1529336306, "message": "some commit", "parents": ["c4e2cdb652d279c6f4078029d70f6e82028db9ff"], "bookmarks": [] },
  >       { "node": "81e4fb4dba036a04d43c6bce369bb0b77b92e0af", "phase": "public", "author": "Test User", "date": 1518430888, "message": "some commit", "parents": ["b4c720b94ee958dc1bca9b26e295ed47bc69b2ee"], "bookmarks": [] },
  >       { "node": "941218e740b1d012cff9f9ea77adddbc6c224e4c", "phase": "draft", "author": "Test User", "date": 1529248423, "message": "some commit", "parents": ["e4061744718293aad95ef95a1154c54278cc9f96"], "bookmarks": [] },
  >       { "node": "97dbf0a5c6ad297f7138393a9c543c89592fef15", "phase": "draft", "author": "Test User", "date": 1514895955, "message": "some commit", "parents": ["073f9863817170dd25574c86fcb1325422711e21"], "bookmarks": [] },
  >       { "node": "9ecb3ee1576c5060da7dea93da0af85f0add2230", "phase": "public", "author": "Test User", "date": 1529331413, "message": "some commit", "parents": ["393570e06fcced494e88afa101d2a0bc59587418"], "bookmarks": [] },
  >       { "node": "a35c2de4bbd0fa50f2de1fd5e80e3fc51a131efa", "phase": "draft", "author": "Test User", "date": 1529336306, "message": "some commit", "parents": ["18ead8dcb4016197962986b9a10317ec6b7e5535"], "bookmarks": [] },
  >       { "node": "a9585631d7ab61b0902cd6ce0abb3029ff663a8f", "phase": "draft", "author": "Test User", "date": 1528735758, "message": "some commit", "parents": ["4c1ab442d10467487ecdaa07189e4c93a7501e93"], "bookmarks": [] },
  >       { "node": "adedd42579c3eb1b15a73adc3ba05d3b635ae05a", "phase": "draft", "author": "Test User", "date": 1526982138, "message": "some commit", "parents": ["5c518c325594dc279b3068133a4b41f4562844c7"], "bookmarks": [] },
  >       { "node": "ae48faf1844bfc184b7897d27594f251a9b627dc", "phase": "draft", "author": "Test User", "date": 1514895955, "message": "some commit", "parents": ["97dbf0a5c6ad297f7138393a9c543c89592fef15"], "bookmarks": [] },
  >       { "node": "b545c3f4a799ccd5651a6ea28e38b63a5330acf6", "phase": "draft", "author": "Test User", "date": 1529334413, "message": "some commit", "parents": ["9ecb3ee1576c5060da7dea93da0af85f0add2230"], "bookmarks": [] },
  >       { "node": "bbb2de39de046fbbd60589886779ef2f9fd2f383", "phase": "draft", "author": "Test User", "date": 1526981133, "message": "some commit", "parents": ["43b76c388a8e78d8b08073cc9d0989cde7a0c4dd"], "bookmarks": [] },
  >       { "node": "c4e2cdb652d279c6f4078029d70f6e82028db9ff", "phase": "draft", "author": "Test User", "date": 1529248423, "message": "some commit", "parents": ["25321d75dd6661cbff80d8c983a84954758ac53f"], "bookmarks": [] },
  >       { "node": "d50e126ac2a6eda9b5a1121c32bf0bf89de58698", "phase": "draft", "author": "Test User", "date": 1529336306, "message": "some commit", "parents": ["192484cd863eeabc3f07520efe24c5530d0abd10"], "bookmarks": [] },
  >       { "node": "d982aa936917d880a40a250ec1aa4250af062cba", "phase": "draft", "author": "Test User", "date": 1529336306, "message": "some commit", "parents": ["5fdfe29f34cd4d45c23ece398adc684343efd46e"], "bookmarks": [] },
  >       { "node": "e4061744718293aad95ef95a1154c54278cc9f96", "phase": "draft", "author": "Test User", "date": 1529248423, "message": "some commit", "parents": ["22bc0fbef62362d9d7f462c21b1ebccd08a47509"], "bookmarks": [] }]
  >   }
  > }
  > EOF

  $ hg cloud sl
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog:
  
    o  b545c3  Test User 2018-06-18 15:06 +0000
  ╭─╯  some commit
  │
  o  9ecb3e (public)  2018-06-18 14:16 +0000
  ╷  some commit
  ╷
  ╷ o  4ac122  Test User 2018-06-18 15:38 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  2ad8ad  Test User 2018-06-18 15:38 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  a35c2d  Test User 2018-06-18 15:38 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  18ead8  Test User 2018-06-18 15:38 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  343314  Test User 2018-06-18 15:38 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  d50e12  Test User 2018-06-18 15:38 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  192484  Test User 2018-06-18 15:38 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  d982aa  Test User 2018-06-18 15:38 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  5fdfe2  Test User 2018-06-18 15:38 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  7e8e6a  Test User 2018-06-18 15:38 +0000
  ╷ │  some commit
  ╷ │
  ╷ │ o  2b9a52  Test User 2018-06-17 15:13 +0000
  ╷ │ │  some commit
  ╷ │ │
  ╷ │ o  5c45cd  Test User 2018-06-17 15:13 +0000
  ╷ │ │  some commit
  ╷ │ │
  ╷ │ o  25d91f  Test User 2018-06-17 15:13 +0000
  ╷ ├─╯  some commit
  ╷ │
  ╷ o  c4e2cd  Test User 2018-06-17 15:13 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  25321d  Test User 2018-06-17 15:13 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  941218  Test User 2018-06-17 15:13 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  e40617  Test User 2018-06-17 15:13 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  22bc0f  Test User 2018-06-17 15:13 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  05e828  Test User 2018-06-17 15:13 +0000
  ╭─╯  some commit
  │
  o  098db6 (public)  2018-06-17 15:12 +0000
  ╷  some commit
  ╷
  ╷ o  a95856  Test User 2018-06-11 16:49 +0000
  ╭─╯  some commit
  │
  o  4c1ab4 (public)  2018-06-11 16:46 +0000
  ╷  some commit
  ╷
  ╷ o  506117  Test User 2018-06-13 14:57 +0000
  ╭─╯  some commit
  │
  o  41c3c6 (public)  2018-06-07 14:41 +0000
  ╷  some commit
  ╷
  ╷ o  bbb2de  Test User 2018-05-22 09:25 +0000
  ╭─╯  some commit
  │
  o  43b76c (public)  2018-05-21 15:05 +0000
  ╷  some commit
  ╷
  ╷ o  adedd4  Test User 2018-05-22 09:42 +0000
  ╭─╯  some commit
  │
  o  5c518c (public)  2018-05-21 14:57 +0000
  ╷  some commit
  ╷
  ╷ o  32f304  Test User 2018-03-02 22:55 +0000
  ╭─╯  some commit
  │
  o  76159c (public)  2018-03-02 22:43 +0000
  ╷  some commit
  ╷
  ╷ o  6e4d02  Test User 2018-02-12 14:22 +0000
  ╭─╯  some commit
  │
  o  81e4fb (public)  2018-02-12 10:21 +0000
  ╷  some commit
  ╷
  ╷ o  0f6762  Test User 2018-01-02 12:25 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  ae48fa  Test User 2018-01-02 12:25 +0000
  ╷ │  some commit
  ╷ │
  ╷ o  97dbf0  Test User 2018-01-02 12:25 +0000
  ╭─╯  some commit
  │
  o  073f98 (public)  2018-01-02 10:00 +0000
     some commit

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
 
  $ hg cloud sl -T {"node"}
  the repository is not connected to any workspace, assuming the 'default' workspace
  commitcloud: searching draft commits for the 'user/test/default' workspace for the 'server' repo
  Smartlog:
  
    o  773bd8234d94c44079b4409525028517fcbd98ba
  ╭─╯
  o  c609e6238e05accd090222c74a0699238f394ba4
  ╷
  ╷ o  685a62272258b3bd4d71ac0b331486276b3c2599
  ╷ │
  ╷ o  aa84f0443f949a6accca6d67b2790d2f37927451
  ╭─╯
  o  99d5fb5998e4f0a77a6b867ddeee93e7666e76c6
  ╷
  ╷ o  717dccd1a732f794c51df27f7ba143c5c743d770
  ╭─╯
  o  30443c40415321c0157d3798f14c51068edb428d
  ╷
  ╷ o  0067e44d36d919bec1bff6ac65d277e8e0dc2250
  ╭─╯
  o  4b1141993451c32f5e1c285ddc88468255cdccf2


