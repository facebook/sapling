#chg-compatible

  $ configure modern
  $ showgraph() {
  >    hg log -G -T "{rev} {desc}: {phase} {bookmarks} {remotenames}" -r "all()"
  > }

  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg commit -Aqm "$1"
  > }

  $ newserver server
  $ cd $TESTTMP/server

  $ mkcommit "base"
  $ hg bookmark master
  $ cd ..

Make the first clone of the server
  $ clone server client1
  $ cd client1
  $ hg cloud rename -d w1 # renaming of the default one should fail
  abort: rename of the default workspace is not allowed
  [255]
  $ hg cloud leave -q
  $ hg cloud join -w w1
  commitcloud: this repository is now connected to the 'user/test/w1' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w1'
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Make the second clone of the server
  $ clone server client2
  $ cd client2
  $ mkcommit "A (W2)"
  $ mkcommit "B (W2)"
  $ hg cloud leave -q
  $ hg cloud join -w w2
  commitcloud: this repository is now connected to the 'user/test/w2' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w2'
  backing up stack rooted at 90a3dff49daa
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 2 commits:
  remote:     90a3dff49daa  A (W2)
  remote:     67590a46a20b  B (W2)

  $ cd ..

Make a commit in the first client, and sync it
  $ cd client1
  $ mkcommit "A (W1)"
  $ mkcommit "B (W1)"
  $ hg cloud sync -q

  $ hg cloud list
  commitcloud: searching workspaces for the 'server' repo
  the following commitcloud workspaces are available:
          default
          w2
          w1 (connected)
  run `hg cloud sl -w <workspace name>` to view the commits
  run `hg cloud switch -w <workspace name>` to switch to a different workspace

Rename to the existing workspace should fail 
  $ hg cloud rename -d w2
  commitcloud: synchronizing 'server' with 'user/test/w1'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: rename the 'user/test/w1' workspace to 'user/test/w2' for the repo 'server'
  abort: workspace: 'user/test/w2' already exists
  [255]


Rename to a new name should work
Smartlog and status should stay the same
  $ hg cloud rename -d w3
  commitcloud: synchronizing 'server' with 'user/test/w1'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: rename the 'user/test/w1' workspace to 'user/test/w3' for the repo 'server'
  commitcloud: rename successful

  $ showgraph
  @  2 B (W1): draft
  |
  o  1 A (W1): draft
  |
  o  0 base: public  remote/master
  
  $ hg cloud sync --debug
  commitcloud: synchronizing 'server' with 'user/test/w3'
  commitcloud local service: get_references for current version 2
  commitcloud: commits synchronized
  finished in * (glob)

Rename workspace that is not a current one
  $ hg cloud rename -s w2 -d w4
  commitcloud: rename the 'user/test/w2' workspace to 'user/test/w4' for the repo 'server'
  commitcloud: rename successful

  $ cd ..

Move to the second client
`hg cloud sync` should be broken
  $ cd client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/w2'
  abort: 'get_references' failed, the workspace has been renamed or client has an invalid state
  [255]

  $ hg cloud leave
  commitcloud: this repository is now disconnected from the 'user/test/w2' workspace
  $ hg cloud list
  commitcloud: searching workspaces for the 'server' repo
  the following commitcloud workspaces are available:
          default
          w3
          w4
  run `hg cloud sl -w <workspace name>` to view the commits
  run `hg cloud switch -w <workspace name>` to switch to a different workspace
  $ hg cloud join -w w4
  commitcloud: this repository is now connected to the 'user/test/w4' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w4'
  commitcloud: commits synchronized
  finished in * (glob)

  $ showgraph
  @  2 B (W2): draft
  |
  o  1 A (W2): draft
  |
  o  0 base: public  remote/master
  

  $ hg cloud rename --rehost -d testhost
  abort: 'rehost' option and 'destination' option are incompatible
  [255]
  $ hg cloud rename --rehost
  commitcloud: synchronizing 'server' with 'user/test/w4'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: rename the 'user/test/w4' workspace to 'user/test/testhost' for the repo 'server'
  commitcloud: rename successful

  $ cd ..

Back to client1  

  $ cd client1
  $ hg up master
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg cloud switch -w testhost # switch to a renamed workspace should work
  commitcloud: synchronizing 'server' with 'user/test/w3'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/w3' to the 'user/test/testhost' workspace
  commitcloud: this repository is now connected to the 'user/test/testhost' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/testhost'
  pulling 67590a46a20b from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  commitcloud: commits synchronized
  finished in * (glob)

  $ showgraph
  o  4 B (W2): draft
  |
  o  3 A (W2): draft
  |
  @  0 base: public  remote/master
  
 
Try to rename an unknown workspace
  $ hg cloud rename -s abc -d w5
  commitcloud: rename the 'user/test/abc' workspace to 'user/test/w5' for the repo 'server'
  abort: unknown workspace: user/test/abc
  [255]
