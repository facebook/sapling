# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig remotefilelog.write-hgcache-to-indexedlog=False remotefilelog.write-local-to-indexedlog=False

define an extension that reveals when Mercurial is fixing up linkrevs

  $ cat > $TESTTMP/loglinkrevfixup.py <<EOF
  > def uisetup(ui):
  >     class loglinkrevfixup(ui.__class__):
  >         def log(self, event, *msg, **opts):
  >             if event == "linkrevfixup":
  >                 self.write("linkrevfixup: %s %s\n" % (opts.get("filepath"), opts.get("fnode")))
  >             return super(loglinkrevfixup, self).log(event, *msg, **opts)
  >     ui.__class__ = loglinkrevfixup
  > EOF

setup configuration
  $ INFINITEPUSH_ALLOW_WRITES=true setup_common_config
  $ cd $TESTTMP

setup repo
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > treeonly = true
  > EOF
  $ echo "content0" > file
  $ hg commit -Aqm base
  $ hg bookmark master_bookmark -r tip

setup repo-push and repo-pull
  $ cd $TESTTMP
  $ for name in push pull1 pull2 pull3
  > do
  >   hgclone_treemanifest ssh://user@dummy/repo-hg repo-$name --noupdate --config extensions.remotenames= --config treemanifest.treeonly=true
  >   cat >> repo-$name/.hg/hgrc <<EOF
  > [extensions]
  > loglinkrevfixup = $TESTTMP/loglinkrevfixup.py
  > infinitepush =
  > commitcloud =
  > remotenames =
  > [infinitepush]
  > branchpattern = re:scratch/.*
  > [treemanifest]
  > treeonly = true
  > EOF
  > done

blobimport
  $ blobimport repo-hg/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
push an infinitepush commit with new content
  $ cd $TESTTMP/repo-push
  $ hg up master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "content1" > file
  $ hg commit -q -m branch
  $ hgmn cloud backup
  backing up stack rooted at 60ab8a6c8e65
  commitcloud: backed up 1 commit
  $ hg log -G -T '{node} {desc} ({remotenames})\n' -r "all()"
  @  60ab8a6c8e652ea968be7ffdb658b49de35d3621 branch ()
  │
  o  d998012a9c34a2423757a3d40f8579c78af1b342 base (default/master_bookmark)
  

pull the infinitepush commit
  $ cd $TESTTMP/repo-pull1
  $ hgmn pull -r 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hgmn up 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg debughistorypack ../cachepath/repo-pull1/packs/f361a1ed16f4b87bbe47e638d8c2cc9f1de8e06f
  
  file
  Node          P1 Node       P2 Node       Link Node     Copy From
  b4aa7b980f00  599997c6080f  000000000000  000000000000  
  599997c6080f  000000000000  000000000000  d998012a9c34  

  $ hg debughistorypack ../cachepath/repo-pull1/packs/manifests/0a557814daab121c2043c7ba26a89a0d60671de6.histpack
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  10ec57c4d9f1  6d825d076849  000000000000  000000000000  

NOTE: Mononoke gave us a NULL linknode

  $ echo othercontent > file2
  $ hg commit -Aqm other
  $ hg log -T '{node} {desc} ({remotenames})\n' -f file
  linkrevfixup: file b4aa7b980f00bcd3ea58510798c1425dcdc511f3
  60ab8a6c8e652ea968be7ffdb658b49de35d3621 branch ()
  d998012a9c34a2423757a3d40f8579c78af1b342 base (default/master_bookmark)

NOTE: linkrevfixup was called to fix up the null linkrev

push a master commit with the same content
  $ cd $TESTTMP/repo-push
  $ hg up master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "content1" > file
  $ hg commit -q -m master
  $ hgmn push --to master_bookmark
  pushing rev 6dbc3093b595 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

pull only the master branch into another repo
  $ cd $TESTTMP/repo-pull2
  $ hg up master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hgmn pull mononoke://$(mononoke_address)/repo -B master_bookmark
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hgmn up master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -G -T '{node} {desc} ({remotenames})\n' -r "all()"
  @  6dbc3093b5955d7bb47512155149ec66791c277d master (default/master_bookmark)
  │
  o  d998012a9c34a2423757a3d40f8579c78af1b342 base ()
  
  $ hg debughistorypack ../cachepath/repo-pull2/packs/e5e1a8b81e9d2360fe54412f8370812c06c6cadb
  
  file
  Node          P1 Node       P2 Node       Link Node     Copy From
  b4aa7b980f00  599997c6080f  000000000000  6dbc3093b595  
  599997c6080f  000000000000  000000000000  d998012a9c34  

  $ hg debughistorypack ../cachepath/repo-pull2/packs/manifests/d4f69b796da6848a455a916d75afe6b27e774058.histpack
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  10ec57c4d9f1  6d825d076849  000000000000  6dbc3093b595  

NOTE: the linknode is the public commit

  $ echo othercontent > file2
  $ hg commit -Aqm other
  $ hg log -T '{node} {desc} ({remotenames})\n' -f file
  6dbc3093b5955d7bb47512155149ec66791c277d master (default/master_bookmark)
  d998012a9c34a2423757a3d40f8579c78af1b342 base ()

NOTE: linkrevfixup was not called

pull the infinitepush commit again in a new repo
  $ cd $TESTTMP/repo-pull3
  $ hgmn pull -r 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hgmn up 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debughistorypack ../cachepath/repo-pull2/packs/e5e1a8b81e9d2360fe54412f8370812c06c6cadb
  
  file
  Node          P1 Node       P2 Node       Link Node     Copy From
  b4aa7b980f00  599997c6080f  000000000000  6dbc3093b595  
  599997c6080f  000000000000  000000000000  d998012a9c34  

  $ hg debughistorypack ../cachepath/repo-pull3/packs/manifests/d4f69b796da6848a455a916d75afe6b27e774058.histpack
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  10ec57c4d9f1  6d825d076849  000000000000  6dbc3093b595  

NOTE: Mononoke gave us the public commit as the linknode

  $ echo othercontent > file2
  $ hg commit -Aqm other
  $ hg log -T '{node} {desc} ({remotenames})\n' -f file
  linkrevfixup: file b4aa7b980f00bcd3ea58510798c1425dcdc511f3
  60ab8a6c8e652ea968be7ffdb658b49de35d3621 branch ()
  d998012a9c34a2423757a3d40f8579c78af1b342 base ()

NOTE: linkrevfixup was called to fix up the linkrev
