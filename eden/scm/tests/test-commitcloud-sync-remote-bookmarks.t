#chg-compatible

  $ enable amend commitcloud infinitepush remotenames
  $ disable treemanifest
  $ configure dummyssh
  $ setconfig remotenames.autopullhoistpattern=re:.*
  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=server

  $ mkcommit() {
  >    echo $1 > $1
  >    hg add $1
  >    hg ci -m "$1"
  > }
  $ showgraph() {
  >    hg log -G -T "{desc}: {phase} {bookmarks} {remotenames}"
  > }

Setup remote repo
  $ hg init remoterepo
  $ cd remoterepo
  $ setconfig infinitepush.server=yes infinitepush.reponame=testrepo
  $ setconfig infinitepush.indextype=disk infinitepush.storetype=disk

  $ mkcommit root
  $ ROOT=$(hg log -r . -T{node})
  $ mkcommit c1 serv
  $ hg book warm
  $ hg up $ROOT -q
  $ mkcommit b1 serv
  $ hg book stable

  $ hg up $ROOT -q
  $ mkcommit a1 serv
  $ mkcommit a2 serv
  $ hg book master

  $ showgraph
  @  a2: draft master
  |
  o  a1: draft
  |
  | o  b1: draft stable
  |/
  | o  c1: draft warm
  |/
  o  root: draft
  

Setup first client repo
  $ cd ..
  $ setconfig remotenames.selectivepull=True
  $ setconfig remotenames.selectivepulldefault=master
  $ setconfig remotenames.selectivepullaccessedbookmarks=True
  $ setconfig commitcloud.remotebookmarkssync=True

  $ hg clone -q ssh://user@dummy/remoterepo client1
  $ cd client1
  $ hg pull -B stable -B warm -q
  $ hg up 'desc(a2)' -q
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ showgraph
  @  a2: public  default/master
  |
  o  a1: public
  |
  | o  b1: public  default/stable
  |/
  | o  c1: public  default/warm
  |/
  o  root: public
  
Setup second client repo
  $ cd ..
  $ hg clone -q ssh://user@dummy/remoterepo client2
  $ cd client2
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec

Setup third client repo but do not enable remote bookmarks sync
  $ cd ..
  $ hg clone -q ssh://user@dummy/remoterepo client3
  $ cd client3
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
  $ setconfig commitcloud.remotebookmarkssync=False
  $ hg cloud join -q

Common case of unsynchronized remote bookmarks
  $ cd ../remoterepo
  $ mkcommit a3 serv
  $ cd ../client2
  $ hg pull -q
  $ hg up master -q
  $ mkcommit draft-1
  $ hg cloud sync -q
  $ showgraph
  @  draft-1: draft
  |
  o  a3: public  default/master
  |
  o  a2: public
  |
  o  a1: public
  |
  o  root: public
  

default/master should point to the new commit
  $ cd ../client1
  $ hg cloud sync -q
  $ showgraph
  o  draft-1: draft
  |
  o  a3: public  default/master
  |
  @  a2: public
  |
  o  a1: public
  |
  o  root: public
  
Subscribe to a new remote bookmark
  $ cd ../client1
  $ hg pull -q
  $ hg pull -B stable -q
  $ hg cloud sync -q
  $ showgraph
  o  draft-1: draft
  |
  o  a3: public  default/master
  |
  @  a2: public
  |
  o  a1: public
  |
  | o  b1: public  default/stable
  |/
  | o  c1: public  default/warm
  |/
  o  root: public
  
  $ hg book --list-subscriptions
     default/master            5:1b6e90080435
     default/stable            2:b2bfab231667
     default/warm              1:b8063fc7de93

the other client should be subscribed to this bookmark as well
  $ cd ../client2
  $ hg cloud sync -q
  $ showgraph
  @  draft-1: draft
  |
  o  a3: public  default/master
  |
  o  a2: public
  |
  o  a1: public
  |
  | o  b1: public  default/stable
  |/
  | o  c1: public  default/warm
  |/
  o  root: public
  
  $ hg book --list-subscriptions
     default/master            5:1b6e90080435
     default/stable            2:b2bfab231667
     default/warm              1:b8063fc7de93

try to create a commit on top of the default/stable
  $ cd ../client1
  $ hg up stable -q
  $ mkcommit draft-2
  $ hg cloud sync -q

  $ cd ../client2
  $ hg cloud sync -q
  $ showgraph
  o  draft-2: draft
  |
  | @  draft-1: draft
  | |
  | o  a3: public  default/master
  | |
  | o  a2: public
  | |
  | o  a1: public
  | |
  o |  b1: public  default/stable
  |/
  | o  c1: public  default/warm
  |/
  o  root: public
  
check that copy with disabled remote bookmarks sync doesn't affect the other copies
  $ cd ../client1
  $ hg up warm -q
  $ mkcommit draft-3
  $ hg cloud sync -q
  $ showgraph
  @  draft-3: draft
  |
  | o  draft-2: draft
  | |
  | | o  draft-1: draft
  | | |
  | | o  a3: public  default/master
  | | |
  | | o  a2: public
  | | |
  | | o  a1: public
  | | |
  | o |  b1: public  default/stable
  | |/
  o /  c1: public  default/warm
  |/
  o  root: public
  
sync and create a new commit on top of the draft-3
  $ cd ../client3
  $ hg cloud sync -q
  $ hg up dc05efd94c6626ddd820e8d98b745ad6b50b82fc -q
  $ echo check >> check
  $ hg commit -qAm "draft-4"
  $ showgraph
  @  draft-4: draft
  |
  o  draft-2: draft
  |
  | o  draft-1: draft
  | |
  | | o  draft-3: draft
  | | |
  | o |  a3: draft
  | | |
  | o |  a2: public  default/master
  | | |
  | o |  a1: public
  | | |
  o | |  b1: draft
  |/ /
  | o  c1: draft
  |/
  o  root: public
  
  $ hg cloud sync -q

  $ cd ../client2
  $ hg cloud sync -q
  $ showgraph
  o  draft-4: draft
  |
  | o  draft-3: draft
  | |
  o |  draft-2: draft
  | |
  | | @  draft-1: draft
  | | |
  | | o  a3: public  default/master
  | | |
  | | o  a2: public
  | | |
  | | o  a1: public
  | | |
  o---+  b1: public  default/stable
   / /
  o /  c1: public  default/warm
  |/
  o  root: public
  
