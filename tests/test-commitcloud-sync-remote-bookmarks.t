  $ enable amend commitcloud infinitepush remotenames
  $ setconfig extensions.treemanifest=!
  $ setconfig ui.ssh="python \"$TESTDIR/dummyssh\""

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
  >    hg log -G -T "{desc}: {phase} {bookmarks} {remotenames}"
  > }

Setup remote repo
  $ hg init remoterepo
  $ cd remoterepo
  $ setconfig infinitepush.server=yes infinitepush.reponame=testrepo
  $ setconfig infinitepush.indextype=disk infinitepush.storetype=disk

  $ mkcommit root
  $ ROOT=$(hg log -r . -T{node})
  $ mkcommit b1 serv
  $ hg book stable

  $ hg up $ROOT -q
  $ mkcommit a1 serv
  $ mkcommit a2 serv
  $ hg book master

  $ showgraph
  @  a2: public master
  |
  o  a1: public
  |
  | o  b1: public stable
  |/
  o  root: public
  

Setup first client repo
  $ cd ..
  $ setconfig remotenames.selectivepull=True
  $ setconfig remotenames.selectivepulldefault=master
  $ hg clone -q ssh://user@dummy/remoterepo client1
  $ cd client1
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ hg cloud auth -t xxxxxx
  setting authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'reponame-default' repo
  commitcloud: synchronizing 'reponame-default' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ showgraph
  @  a2: public  default/master
  |
  o  a1: public
  |
  o  root: public
  
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
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'reponame-default' repo
  commitcloud: synchronizing 'reponame-default' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
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
(it doen't work because remote bookmarks sync wasn't enabled)
  $ cd ../client1
  $ hg cloud sync -q
  $ showgraph
  o  draft-1: draft
  |
  o  a3: public
  |
  @  a2: public  default/master
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
  o  b1: public  default/stable
  |
  | o  draft-1: draft
  | |
  | o  a3: public  default/master
  | |
  | @  a2: public
  | |
  | o  a1: public
  |/
  o  root: public
  
  $ hg book --list-subscriptions
     default/master            3:1b6e90080435
     default/stable            5:b2bfab231667

the other client should be subscribed to this bookmark as well
(it doen't work because remote bookmarks sync wasn't enabled)
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
  o  root: public
  
  $ hg book --list-subscriptions
     default/master            3:1b6e90080435

try to create a commit on top of the default/stable
(remote bookmark is still not here)
  $ cd ../client1
  $ hg up stable -q
  $ mkcommit draft-2
  $ hg cloud sync -q

  $ cd ../client2
  $ hg cloud sync -q
  $ showgraph
  o  draft-2: draft
  |
  o  b1: public
  |
  | @  draft-1: draft
  | |
  | o  a3: public  default/master
  | |
  | o  a2: public
  | |
  | o  a1: public
  |/
  o  root: public
  
