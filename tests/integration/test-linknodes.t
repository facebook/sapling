  $ . $TESTDIR/library.sh

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
  $ setup_common_config
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
  > infinitepushbackup =
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
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

push an infinitepush commit with new content
  $ cd $TESTTMP/repo-push
  $ hg up master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "content1" > file
  $ hg commit -q -m branch
  $ hgmn pushbackup
  starting backup * (glob)
  backing up stack rooted at 60ab8a6c8e65
  remote: * DEBG Session with Mononoke started * (glob)
  finished in * seconds (glob)
  $ hg log -G -T '{node} {desc} ({remotenames})\n' -r "all()"
  @  60ab8a6c8e652ea968be7ffdb658b49de35d3621 branch ()
  |
  o  d998012a9c34a2423757a3d40f8579c78af1b342 base (default/master_bookmark default/default)
  

pull the infinitepush commit
  $ cd $TESTTMP/repo-pull1
  $ hgmn pull -r 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 60ab8a6c8e65
  (run 'hg update' to get a working copy)
  $ hgmn up 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  remote: * DEBG Session with Mononoke started * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugremotefilelog ../cachepath/repo-pull1/97/1c419dd609331343dee105fffd0f4608dc0bf2/b4aa7b980f00bcd3ea58510798c1425dcdc511f3
  size: 9 bytes
  path: ../cachepath/repo-pull1/97/1c419dd609331343dee105fffd0f4608dc0bf2/b4aa7b980f00bcd3ea58510798c1425dcdc511f3 
  key: b4aa7b980f00 
  filename: file 
  
          node =>           p1            p2      linknode     copyfrom
  b4aa7b980f00 => 599997c6080f  000000000000  60ab8a6c8e65  
  599997c6080f => 000000000000  000000000000  d998012a9c34  

  $ hg debughistorypack ../cachepath/repo-pull1/packs/manifests/591d1ef6950c4a063802cc9cf4d4549022bf86e2.histpack
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  10ec57c4d9f1  6d825d076849  000000000000  60ab8a6c8e65  

NOTE: Mononoke gave us the draft commit as the linknode

push a master commit with the same content
  $ cd $TESTTMP/repo-push
  $ hg up master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "content1" > file
  $ hg commit -q -m master
  $ hgmn push ssh://user@dummy/repo --to master_bookmark
  remote: * DEBG Session with Mononoke started * (glob)
  pushing rev 6dbc3093b595 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

pull only the master branch into another repo
  $ cd $TESTTMP/repo-pull2
  $ hg up master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hgmn pull ssh://user@dummy/repo -B master_bookmark
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 6dbc3093b595
  (run 'hg update' to get a working copy)
  $ hgmn up master_bookmark
  remote: * DEBG Session with Mononoke started * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -T '{node} {desc} ({remotenames})\n' -r "all()"
  @  6dbc3093b5955d7bb47512155149ec66791c277d master (default/master_bookmark)
  |
  o  d998012a9c34a2423757a3d40f8579c78af1b342 base ()
  
  $ hg debugremotefilelog ../cachepath/repo-pull2/97/1c419dd609331343dee105fffd0f4608dc0bf2/b4aa7b980f00bcd3ea58510798c1425dcdc511f3
  size: 9 bytes
  path: ../cachepath/repo-pull2/97/1c419dd609331343dee105fffd0f4608dc0bf2/b4aa7b980f00bcd3ea58510798c1425dcdc511f3 
  key: b4aa7b980f00 
  filename: file 
  
          node =>           p1            p2      linknode     copyfrom
  b4aa7b980f00 => 599997c6080f  000000000000  60ab8a6c8e65  
  599997c6080f => 000000000000  000000000000  d998012a9c34  

  $ hg debughistorypack ../cachepath/repo-pull2/packs/manifests/591d1ef6950c4a063802cc9cf4d4549022bf86e2.histpack
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  10ec57c4d9f1  6d825d076849  000000000000  60ab8a6c8e65  

PROBLEM: the linknode is 60ab8a6c8e65, which we don't have locally

  $ echo othercontent > file2
  $ hg commit -Aqm other
  $ hg log -T '{node} {desc} ({remotenames})\n' -f file
  linkrevfixup: file b4aa7b980f00bcd3ea58510798c1425dcdc511f3
  6dbc3093b5955d7bb47512155149ec66791c277d master (default/master_bookmark)
  d998012a9c34a2423757a3d40f8579c78af1b342 base ()

PROBLEM: linkrevfixup was called

pull the infinitepush commit again in a new repo
  $ cd $TESTTMP/repo-pull3
  $ hgmn pull -r 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 60ab8a6c8e65
  (run 'hg update' to get a working copy)
  $ hgmn up 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  remote: * DEBG Session with Mononoke started * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugremotefilelog ../cachepath/repo-pull3/97/1c419dd609331343dee105fffd0f4608dc0bf2/b4aa7b980f00bcd3ea58510798c1425dcdc511f3
  size: 9 bytes
  path: ../cachepath/repo-pull3/97/1c419dd609331343dee105fffd0f4608dc0bf2/b4aa7b980f00bcd3ea58510798c1425dcdc511f3 
  key: b4aa7b980f00 
  filename: file 
  
          node =>           p1            p2      linknode     copyfrom
  b4aa7b980f00 => 599997c6080f  000000000000  60ab8a6c8e65  
  599997c6080f => 000000000000  000000000000  d998012a9c34  

  $ hg debughistorypack ../cachepath/repo-pull3/packs/manifests/591d1ef6950c4a063802cc9cf4d4549022bf86e2.histpack
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  10ec57c4d9f1  6d825d076849  000000000000  60ab8a6c8e65  

NOTE: Mononoke gave us the draft commit as the linknode

