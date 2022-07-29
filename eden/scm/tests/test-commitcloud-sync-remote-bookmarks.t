#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ enable amend commitcloud infinitepush remotenames
  $ configure dummyssh
  $ setconfig remotenames.autopullhoistpattern=re:.*
  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=server
  $ setconfig devel.segmented-changelog-rev-compat=true

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
  $ hg book main

  $ hg up $ROOT -q
  $ mkcommit a1 serv
  $ mkcommit a2 serv
  $ hg book master

  $ showgraph
  @  a2: draft master
  │
  o  a1: draft
  │
  │ o  b1: draft main stable
  ├─╯
  │ o  c1: draft warm
  ├─╯
  o  root: draft
  

Setup first client repo and subscribe to the bookmarks "stable" and "warm".
  $ cd ..
  $ setconfig remotenames.selectivepull=True
  $ setconfig remotenames.selectivepulldefault=master
  $ setconfig commitcloud.remotebookmarkssync=True

  $ hg clone -q ssh://user@dummy/remoterepo client1
  $ cd client1
  $ hg pull -B stable -B warm -q
  $ hg up 'desc(a2)' -q
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
  $ hg cloud join -q
  $ showgraph
  @  a2: public  default/master
  │
  o  a1: public
  │
  │ o  b1: public  default/stable
  ├─╯
  │ o  c1: public  default/warm
  ├─╯
  o  root: public
  
Setup the second client repo with enable remote bookmarks sync
The repo should be subscribed the "stable" and "warm" bookmark because the client1 was.
  $ cd ..
  $ hg clone -q ssh://user@dummy/remoterepo client2
  $ cd client2
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
  $ hg cloud join -q
  $ showgraph
  @  a2: public  default/master
  │
  o  a1: public
  │
  │ o  b1: public  default/stable
  ├─╯
  │ o  c1: public  default/warm
  ├─╯
  o  root: public
  

Setup third client repo but do not enable remote bookmarks sync
  $ cd ..
  $ hg clone -q ssh://user@dummy/remoterepo client3
  $ cd client3
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
  $ setconfig commitcloud.remotebookmarkssync=False
  $ hg cloud join -q
  $ showgraph
  @  a2: public  default/master
  │
  o  a1: public
  │
  o  root: public
  

Common case of unsynchronized remote bookmarks ("master")
  $ cd ../remoterepo
  $ mkcommit a3 serv
  $ cd ../client2
  $ hg pull -q
  $ hg up master -q
  $ mkcommit draft-1
  $ hg cloud sync -q
  $ showgraph
  @  draft-1: draft
  │
  o  a3: public  default/master
  │
  o  a2: public
  │
  o  a1: public
  │
  │ o  b1: public  default/stable
  ├─╯
  │ o  c1: public  default/warm
  ├─╯
  o  root: public
  

default/master should point to the new commit
  $ cd ../client1
  $ hg cloud sync -q
  $ showgraph
  o  draft-1: draft
  │
  o  a3: public  default/master
  │
  @  a2: public
  │
  o  a1: public
  │
  │ o  b1: public  default/stable
  ├─╯
  │ o  c1: public  default/warm
  ├─╯
  o  root: public
  
Subscribe to a new remote bookmark "main" that previously has been only known on the server
  $ cd ../client1
  $ hg pull -q
  $ hg pull -B main -q
  $ hg cloud sync -q
  $ showgraph
  o  draft-1: draft
  │
  o  a3: public  default/master
  │
  @  a2: public
  │
  o  a1: public
  │
  │ o  b1: public  default/main default/stable
  ├─╯
  │ o  c1: public  default/warm
  ├─╯
  o  root: public
  
  $ hg book --list-subscriptions
     default/main              b2bfab231667
     default/master            1b6e90080435
     default/stable            b2bfab231667
     default/warm              b8063fc7de93

the other client should be subscribed to this bookmark ("main") as well
  $ cd ../client2
  $ hg cloud sync -q
  $ showgraph
  @  draft-1: draft
  │
  o  a3: public  default/master
  │
  o  a2: public
  │
  o  a1: public
  │
  │ o  b1: public  default/main default/stable
  ├─╯
  │ o  c1: public  default/warm
  ├─╯
  o  root: public
  
  $ hg book --list-subscriptions
     default/main              b2bfab231667
     default/master            1b6e90080435
     default/stable            b2bfab231667
     default/warm              b8063fc7de93

try to create a commit on top of the default/stable
  $ cd ../client1
  $ hg up stable -q
  $ mkcommit draft-2
  $ hg cloud sync -q

  $ cd ../client2
  $ hg cloud sync -q
  $ showgraph
  o  draft-2: draft
  │
  │ @  draft-1: draft
  │ │
  │ o  a3: public  default/master
  │ │
  │ o  a2: public
  │ │
  │ o  a1: public
  │ │
  o │  b1: public  default/main default/stable
  ├─╯
  │ o  c1: public  default/warm
  ├─╯
  o  root: public
  
check that copy with disabled remote bookmarks sync doesn't affect the other copies
  $ cd ../client1
  $ hg up warm -q
  $ mkcommit draft-3
  $ hg cloud sync -q
  $ showgraph
  @  draft-3: draft
  │
  │ o  draft-2: draft
  │ │
  │ │ o  draft-1: draft
  │ │ │
  │ │ o  a3: public  default/master
  │ │ │
  │ │ o  a2: public
  │ │ │
  │ │ o  a1: public
  │ │ │
  │ o │  b1: public  default/main default/stable
  │ ├─╯
  o │  c1: public  default/warm
  ├─╯
  o  root: public
  
sync and create a new commit on top of the draft-3
  $ cd ../client3
  $ hg cloud sync -q
  $ hg up dc05efd94c6626ddd820e8d98b745ad6b50b82fc -q
  $ echo check >> check
  $ hg commit -qAm "draft-4"
  $ showgraph
  @  draft-4: draft
  │
  o  draft-2: draft
  │
  │ o  draft-1: draft
  │ │
  │ │ o  draft-3: draft
  │ │ │
  │ o │  a3: draft
  │ │ │
  │ o │  a2: public  default/master
  │ │ │
  │ o │  a1: public
  │ │ │
  o │ │  b1: draft
  ├─╯ │
  │   o  c1: draft
  ├───╯
  o  root: public
  
  $ hg cloud sync -q

  $ cd ../client2
  $ hg cloud sync -q
  $ showgraph
  o  draft-4: draft
  │
  │ o  draft-3: draft
  │ │
  o │  draft-2: draft
  │ │
  │ │ @  draft-1: draft
  │ │ │
  │ │ o  a3: public  default/master
  │ │ │
  │ │ o  a2: public
  │ │ │
  │ │ o  a1: public
  │ │ │
  o │ │  b1: public  default/main default/stable
  ├───╯
  │ o  c1: public  default/warm
  ├─╯
  o  root: public
  
