# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

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
  $ hginit_treemanifest repo
  $ cd repo
  $ echo "content0" > file
  $ hg commit -Aqm base
  $ hg bookmark master_bookmark -r tip

setup repo-push and repo-pull
  $ cd $TESTTMP
  $ for name in push pull1 pull2 pull3
  > do
  >   hg clone -q mono:repo repo-$name --noupdate
  >   cat >> repo-$name/.hg/hgrc <<EOF
  > [extensions]
  > loglinkrevfixup = $TESTTMP/loglinkrevfixup.py
  > infinitepush =
  > commitcloud =
  > [infinitepush]
  > branchpattern = re:scratch/.*
  > EOF
  >   
  >   # Defeat shared cache between repos.
  >   cat >> repo-$name/.hg/hgrc <<EOF
  > [remotefilelog]
  > cachepath=$TESTTMP/cachepath/$name
  > EOF
  > done

blobimport
  $ blobimport repo/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
push an infinitepush commit with new content
  $ cd $TESTTMP/repo-push
  $ hg up master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "content1" > file
  $ hg commit -q -m branch
  $ hg cloud backup
  commitcloud: head '60ab8a6c8e65' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  $ hg log -G -T '{node} {desc} ({remotenames})\n' -r "all()"
  @  60ab8a6c8e652ea968be7ffdb658b49de35d3621 branch ()
  │
  o  d998012a9c34a2423757a3d40f8579c78af1b342 base (default/master_bookmark)
  

pull the infinitepush commit
  $ cd $TESTTMP/repo-pull1
  $ hg pull -r 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  pulling from mono:repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg up 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg debugapi -e history -i '[("file", "b4aa7b980f00bcd3ea58510798c1425dcdc511f3")]'
  [{"key": {"node": bin("b4aa7b980f00bcd3ea58510798c1425dcdc511f3"),
            "path": "file"},
    "nodeinfo": {"parents": [{"node": bin("599997c6080f1c12417bbc03894af754eea8dc72"),
                              "path": "file"},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("0000000000000000000000000000000000000000")}},
   {"key": {"node": bin("599997c6080f1c12417bbc03894af754eea8dc72"),
            "path": "file"},
    "nodeinfo": {"parents": [{"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("d998012a9c34a2423757a3d40f8579c78af1b342")}}]

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
  $ hg push --to master_bookmark
  pushing rev 6dbc3093b595 to destination mono:repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

Make sure the server derives the linknode info for public commit.
  $ mononoke_newadmin derived-data -R repo derive -T hgchangesets -i 6dbc3093b5955d7bb47512155149ec66791c277d

pull only the master branch into another repo
  $ cd $TESTTMP/repo-pull2
  $ hg up master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg pull -B master_bookmark
  pulling from mono:repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg up master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -G -T '{node} {desc} ({remotenames})\n' -r "all()"
  @  6dbc3093b5955d7bb47512155149ec66791c277d master (default/master_bookmark)
  │
  o  d998012a9c34a2423757a3d40f8579c78af1b342 base ()
  

  $ hg debugapi -e history -i '[("file", "b4aa7b980f00bcd3ea58510798c1425dcdc511f3")]'
  [{"key": {"node": bin("b4aa7b980f00bcd3ea58510798c1425dcdc511f3"),
            "path": "file"},
    "nodeinfo": {"parents": [{"node": bin("599997c6080f1c12417bbc03894af754eea8dc72"),
                              "path": "file"},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("6dbc3093b5955d7bb47512155149ec66791c277d")}},
   {"key": {"node": bin("599997c6080f1c12417bbc03894af754eea8dc72"),
            "path": "file"},
    "nodeinfo": {"parents": [{"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("d998012a9c34a2423757a3d40f8579c78af1b342")}}]

NOTE: the linknode is the public commit

  $ echo othercontent > file2
  $ hg commit -Aqm other
  $ hg log -T '{node} {desc} ({remotenames})\n' -f file
  6dbc3093b5955d7bb47512155149ec66791c277d master (default/master_bookmark)
  d998012a9c34a2423757a3d40f8579c78af1b342 base ()

NOTE: linkrevfixup was not called

pull the infinitepush commit again in a new repo
  $ cd $TESTTMP/repo-pull3
  $ hg pull -r 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  pulling from mono:repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg up 60ab8a6c8e652ea968be7ffdb658b49de35d3621
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg debugapi -e history -i '[("file", "b4aa7b980f00bcd3ea58510798c1425dcdc511f3")]'
  [{"key": {"node": bin("b4aa7b980f00bcd3ea58510798c1425dcdc511f3"),
            "path": "file"},
    "nodeinfo": {"parents": [{"node": bin("599997c6080f1c12417bbc03894af754eea8dc72"),
                              "path": "file"},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("6dbc3093b5955d7bb47512155149ec66791c277d")}},
   {"key": {"node": bin("599997c6080f1c12417bbc03894af754eea8dc72"),
            "path": "file"},
    "nodeinfo": {"parents": [{"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("d998012a9c34a2423757a3d40f8579c78af1b342")}}]

NOTE: Mononoke gave us the public commit as the linknode

  $ echo othercontent > file2
  $ hg commit -Aqm other
  $ hg log -T '{node} {desc} ({remotenames})\n' -f file
  linkrevfixup: file b4aa7b980f00bcd3ea58510798c1425dcdc511f3
  60ab8a6c8e652ea968be7ffdb658b49de35d3621 branch ()
  d998012a9c34a2423757a3d40f8579c78af1b342 base ()

NOTE: linkrevfixup was called to fix up the linkrev
