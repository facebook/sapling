  $ enable amend commitcloud infinitepush remotenames
  $ setconfig extensions.treemanifest=!
  $ setconfig ui.ssh="python \"$TESTDIR/dummyssh\""
  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=server

  $ mkcommit() {
  >    echo $1 > $1
  >    hg add $1
  >    hg ci -m "$1"
  >    S="serv"
  >    if [ "$2" = "$S" ]; then
  >       hg phase --public .
  >    else
  >       hg phase --draft .
  >    fi
  > }
  $ showgraph() {
  >    hg log -G -T "{rev} {desc}: {phase} {bookmarks} {remotenames}"
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
  @  4 a2: public master
  |
  o  3 a1: public
  |
  | o  2 b1: public stable
  |/
  | o  1 c1: public warm
  |/
  o  0 root: public
  

Setup first client repo
  $ cd ..
  $ setconfig remotenames.selectivepull=True
  $ setconfig remotenames.selectivepulldefault=master
  $ setconfig remotenames.selectivepullaccessedbookmarks=True
  $ setconfig commitcloud.remotebookmarkssync=True

  $ hg clone -q ssh://user@dummy/remoterepo client1
  $ cd client1
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ hg cloud auth -t xxxxxx
  setting authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ showgraph
  @  4 a2: public  default/master
  |
  o  3 a1: public
  |
  | o  2 b1: public
  |/
  | o  1 c1: public
  |/
  o  0 root: public
  
Setup second client repo
  $ cd ..
  $ hg clone -q ssh://user@dummy/remoterepo client2
  $ cd client2
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ hg cloud auth -t xxxxxx
  updating authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec

Setup third client repo but do not enable remote bookmarks sync
  $ cd ..
  $ hg clone -q ssh://user@dummy/remoterepo client3
  $ cd client3
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ setconfig commitcloud.remotebookmarkssync=False
  $ hg cloud auth -t xxxxxx
  updating authentication token
  authentication successful
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
  @  6 draft-1: draft
  |
  o  5 a3: public  default/master
  |
  o  4 a2: public
  |
  o  3 a1: public
  |
  | o  2 b1: public
  |/
  | o  1 c1: public
  |/
  o  0 root: public
  

default/master should point to the new commit
  $ cd ../client1
  $ hg cloud sync -q
  $ showgraph
  o  6 draft-1: draft
  |
  o  5 a3: public  default/master
  |
  @  4 a2: public
  |
  o  3 a1: public
  |
  | o  2 b1: public
  |/
  | o  1 c1: public
  |/
  o  0 root: public
  
Subscribe to a new remote bookmark
  $ cd ../client1
  $ hg pull -q
  $ hg pull -B stable -q
  $ hg cloud sync -q
  $ showgraph
  o  6 draft-1: draft
  |
  o  5 a3: public  default/master
  |
  @  4 a2: public
  |
  o  3 a1: public
  |
  | o  2 b1: public  default/stable
  |/
  | o  1 c1: public
  |/
  o  0 root: public
  
  $ hg book --list-subscriptions
     default/master            5:1b6e90080435
     default/stable            2:b2bfab231667

the other client should be subscribed to this bookmark as well
  $ cd ../client2
  $ hg cloud sync -q
  $ showgraph
  @  6 draft-1: draft
  |
  o  5 a3: public  default/master
  |
  o  4 a2: public
  |
  o  3 a1: public
  |
  | o  2 b1: public  default/stable
  |/
  | o  1 c1: public
  |/
  o  0 root: public
  
  $ hg book --list-subscriptions
     default/master            5:1b6e90080435
     default/stable            2:b2bfab231667

try to create a commit on top of the default/stable
  $ cd ../client1
  $ hg up stable -q
  $ mkcommit draft-2
  $ hg cloud sync -q

  $ cd ../client2
  $ hg cloud sync -q
  $ showgraph
  o  7 draft-2: draft
  |
  | @  6 draft-1: draft
  | |
  | o  5 a3: public  default/master
  | |
  | o  4 a2: public
  | |
  | o  3 a1: public
  | |
  o |  2 b1: public  default/stable
  |/
  | o  1 c1: public
  |/
  o  0 root: public
  
check that copy with disabled remote bookmarks sync doesn't affect the other copies
  $ cd ../client1
  $ hg up warm -q
  `warm` not found: assuming it is a remote bookmark and trying to pull it
  `warm` found remotely
  $ mkcommit draft-3
  $ hg cloud sync -q
  $ showgraph
  @  8 draft-3: draft
  |
  | o  7 draft-2: draft
  | |
  | | o  6 draft-1: draft
  | | |
  | | o  5 a3: public  default/master
  | | |
  | | o  4 a2: public
  | | |
  | | o  3 a1: public
  | | |
  | o |  2 b1: public  default/stable
  | |/
  o /  1 c1: public  default/warm
  |/
  o  0 root: public
  
sync and create a new commit on top of the draft-3
  $ cd ../client3
  $ hg cloud sync -q
  $ hg up 8 -q
  $ echo check >> check
  $ hg commit -qAm "draft-4"
  $ showgraph
  @  9 draft-4: draft
  |
  o  8 draft-3: draft
  |
  | o  7 draft-2: draft
  | |
  | | o  6 draft-1: draft
  | | |
  | | o  5 a3: public
  | | |
  | | o  4 a2: public  default/master
  | | |
  | | o  3 a1: public
  | | |
  | o |  2 b1: public
  | |/
  o /  1 c1: public
  |/
  o  0 root: public
  
  $ hg cloud sync -q

  $ cd ../client2
  $ hg cloud sync -q
  $ showgraph
  o  9 draft-4: draft
  |
  o  8 draft-3: draft
  |
  | o  7 draft-2: draft
  | |
  | | @  6 draft-1: draft
  | | |
  | | o  5 a3: public  default/master
  | | |
  | | o  4 a2: public
  | | |
  | | o  3 a1: public
  | | |
  | o |  2 b1: public  default/stable
  | |/
  o /  1 c1: public  default/warm
  |/
  o  0 root: public
  
