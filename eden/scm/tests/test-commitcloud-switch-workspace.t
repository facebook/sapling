#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ configure modern
  $ showgraph() {
  >    hg log -G -T "{desc}: {phase} {bookmarks} {remotenames}" -r "all()"
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
  $ hg cloud leave -q
  $ hg cloud join -w w2
  commitcloud: this repository is now connected to the 'user/test/w2' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w2'
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Make a commit in the first client, and sync it
  $ cd client1
  $ mkcommit "A (W1)"
  $ mkcommit "B (W1)"
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/w1'
  backing up stack rooted at b624c739a2da
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 2 commits:
  remote:     b624c739a2da  A (W1)
  remote:     aab6fffb2884  B (W1)

  $ cd ..

Make a commit in the second client, and sync it
  $ cd client2
  $ mkcommit "C (W2)"
  $ mkcommit "D (W2)"
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/w2'
  backing up stack rooted at 8440f5b0f1c3
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 2 commits:
  remote:     8440f5b0f1c3  C (W2)
  remote:     dff058cfb955  D (W2)

  $ cd ..

Switch workspace in the first client
  $ cd client1
Switch workspace without specifying merge or switch strategy
  $ hg cloud join -w w1
  commitcloud: this repository has been already connected to the 'user/test/w1' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w1'
  commitcloud: commits synchronized
  finished in * (glob)

Switch workspace to the same workspace
  $ hg cloud join --switch -w w1
  commitcloud: this repository has been already connected to the 'user/test/w1' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w1'
  commitcloud: commits synchronized
  finished in * (glob)

Switch workspace from a draft commit that is an ancestor of a main bookmark
  $ hg cloud join --switch -w w2
  commitcloud: synchronizing 'server' with 'user/test/w1'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/w1' to the 'user/test/w2' workspace
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  working directory now at d20a80d4def3
  commitcloud: this repository is now connected to the 'user/test/w2' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w2'
  pulling dff058cfb955 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ showgraph
  o  D (W2): draft
  │
  o  C (W2): draft
  │
  @  base: public  remote/master
  
 
Switch workspace using merge strategy
  $ hg cloud join -w w2_rename --merge
  abort: this repository can not be switched to the 'user/test/w2_rename' workspace
  the workspace doesn't exist (please use --create option to create the workspace)
  [255]
  $ hg cloud join -w w2_rename --merge --create
  commitcloud: this repository will be reconnected from the 'user/test/w2' to the 'user/test/w2_rename' workspace
  commitcloud: all local commits and bookmarks will be merged into 'user/test/w2_rename' workspace
  commitcloud: this repository is now connected to the 'user/test/w2_rename' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w2_rename'
  commitcloud: commits synchronized
  finished in * (glob)
  $ showgraph
  o  D (W2): draft
  │
  o  C (W2): draft
  │
  @  base: public  remote/master
  

Switch workspace back
  $ hg cloud join -w w1 --switch
  commitcloud: synchronizing 'server' with 'user/test/w2_rename'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/w2_rename' to the 'user/test/w1' workspace
  commitcloud: this repository is now connected to the 'user/test/w1' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w1'
  commitcloud: commits synchronized
  finished in * (glob)
  $ showgraph
  o  B (W1): draft
  │
  o  A (W1): draft
  │
  @  base: public  remote/master
  

Create a bookmark and switch workspace. The bookmark should be preserved in the original workspace
  $ hg bookmark "book (W1)" -r 2
  $ showgraph
  o  B (W1): draft book (W1)
  │
  o  A (W1): draft
  │
  @  base: public  remote/master
  
  $ hg cloud join -w w2 --switch
  commitcloud: synchronizing 'server' with 'user/test/w1'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/w1' to the 'user/test/w2' workspace
  commitcloud: this repository is now connected to the 'user/test/w2' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w2'
  commitcloud: commits synchronized
  finished in * (glob)
  $ showgraph
  o  D (W2): draft
  │
  o  C (W2): draft
  │
  @  base: public  remote/master
  

Create a bookmark in w2 and switch workspace. The bookmark should be preserved in the w2. The w1 bookmark should appear.
  $ hg bookmark "book (W2)" -r 4
  $ showgraph
  o  D (W2): draft book (W2)
  │
  o  C (W2): draft
  │
  @  base: public  remote/master
  

  $ hg cloud join -w w1 --switch
  commitcloud: synchronizing 'server' with 'user/test/w2'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/w2' to the 'user/test/w1' workspace
  commitcloud: this repository is now connected to the 'user/test/w1' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w1'
  commitcloud: commits synchronized
  finished in * (glob)
  $ showgraph
  o  B (W1): draft book (W1)
  │
  o  A (W1): draft
  │
  @  base: public  remote/master
  
  $ cd ..

Switch between workspaces w1 and w2 in client2
  $ cd client2
  $ showgraph
  @  D (W2): draft
  │
  o  C (W2): draft
  │
  o  base: public  remote/master
  
  $ hg up d20a80d4def3
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg cloud join -w w1 --switch
  commitcloud: synchronizing 'server' with 'user/test/w2'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/w2' to the 'user/test/w1' workspace
  commitcloud: this repository is now connected to the 'user/test/w1' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w1'
  pulling aab6fffb2884 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ showgraph
  o  B (W1): draft book (W1)
  │
  o  A (W1): draft
  │
  @  base: public  remote/master
  
  $ hg cloud join -w w2 --switch
  commitcloud: synchronizing 'server' with 'user/test/w1'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/w1' to the 'user/test/w2' workspace
  commitcloud: this repository is now connected to the 'user/test/w2' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w2'
  commitcloud: commits synchronized
  finished in * (glob)
  $ showgraph
  o  D (W2): draft book (W2)
  │
  o  C (W2): draft
  │
  @  base: public  remote/master
  
  $ hg cloud join -w w1 --switch
  commitcloud: synchronizing 'server' with 'user/test/w2'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/w2' to the 'user/test/w1' workspace
  commitcloud: this repository is now connected to the 'user/test/w1' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w1'
  commitcloud: commits synchronized
  finished in * (glob)
  $ showgraph
  o  B (W1): draft book (W1)
  │
  o  A (W1): draft
  │
  @  base: public  remote/master
  
  $ cd ..

Make the third clone of the server
  $ clone server client3
  $ cd client3
  $ hg cloud leave
  commitcloud: this repository is now disconnected from the 'user/test/default' workspace

Try to provide switch and merge options together
  $ hg cloud join -w w1 --switch --merge
  commitcloud: 'switch' and 'merge' options can not be provided together, please choose one over another
  [1]

Join a new workspace
  $ hg cloud join -w3
  commitcloud: this repository is now connected to the 'user/test/3' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/3'
  commitcloud: commits synchronized
  finished in * (glob)

Try to switch with uncommitted changes from a draft commit. So the uncommitted changes doesn't allow to update to the public root.
  $ echo 'hello' > hello.txt
  $ hg add hello.txt
  $ hg commit -m "new file"
  $ echo 'goodbye' > hello.txt
  $ hg cloud join -w w1 --switch
  commitcloud: synchronizing 'server' with 'user/test/3'
  backing up stack rooted at dfa54c832678
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/3' to the 'user/test/w1' workspace
  remote: pushing 1 commit:
  remote:     dfa54c832678  new file
  abort: uncommitted changes
  [255]

Commit changes to be able to switch
  $ hg commit -m "new file"
  $ hg up d20a80d4def3
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg cloud join -w w1 --switch
  commitcloud: synchronizing 'server' with 'user/test/3'
  backing up stack rooted at dfa54c832678
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/3' to the 'user/test/w1' workspace
  commitcloud: this repository is now connected to the 'user/test/w1' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/w1'
  pulling aab6fffb2884 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  remote: pushing 2 commits:
  remote:     dfa54c832678  new file
  remote:     5fe2dc2fae70  new file
  commitcloud: commits synchronized
  finished in * (glob)
  $ showgraph
  o  B (W1): draft book (W1)
  │
  o  A (W1): draft
  │
  @  base: public  remote/master
  
  $ cd ..

Testing switching workspace with different remote bookmarks
  $ cd server
  $ mkcommit "M" # move master
  $ hg prev -q
  [d20a80] base
  $ mkcommit "F"
  $ hg bookmark "feature"
  $ hg prev -q
  [d20a80] base
  $ mkcommit "S"
  $ hg bookmark "stable"

  $ cd ..

  $ cd client1
  $ hg pull -B feature -q
  $ hg pull -B master -q
  $ showgraph
  o  M: public  remote/master
  │
  │ o  F: public  remote/feature
  ├─╯
  │ o  B (W1): draft book (W1)
  │ │
  │ o  A (W1): draft
  ├─╯
  @  base: public
  
 
Bookmark feature should disappear in w2 but master will stay as it is a protected bookmark in this configuration. 
  $ hg cloud join -w w2 --switch -q 
  $ showgraph
  o  M: public  remote/master
  │
  │ o  D (W2): draft book (W2)
  │ │
  │ o  C (W2): draft
  ├─╯
  @  base: public
  

  $ hg pull -B stable -q
  $ showgraph
  o  S: public  remote/stable
  │
  │ o  M: public  remote/master
  ├─╯
  │ o  D (W2): draft book (W2)
  │ │
  │ o  C (W2): draft
  ├─╯
  @  base: public
  

Switch back. Bookmark stable should disappear.
  $ hg cloud join -w w1 --switch -q
  $ showgraph
  o  M: public  remote/master
  │
  │ o  F: public  remote/feature
  ├─╯
  │ o  B (W1): draft book (W1)
  │ │
  │ o  A (W1): draft
  ├─╯
  @  base: public
  

Switch one more time. Bookmark stable should return and feature disappear.
  $ hg cloud join -w w2 --switch -q
  $ showgraph
  o  S: public  remote/stable
  │
  │ o  M: public  remote/master
  ├─╯
  │ o  D (W2): draft book (W2)
  │ │
  │ o  C (W2): draft
  ├─╯
  @  base: public
  

Pull a commit from another workspace
  $ hg pull -r b624c739a2da -q
  $ showgraph
  o  S: public  remote/stable
  │
  │ o  M: public  remote/master
  ├─╯
  │ o  D (W2): draft book (W2)
  │ │
  │ o  C (W2): draft
  ├─╯
  │ o  A (W1): draft
  ├─╯
  @  base: public
  

Switch back to W1

  $ hg cloud join -w w1 --switch -q
  $ showgraph
  o  M: public  remote/master
  │
  │ o  F: public  remote/feature
  ├─╯
  │ o  B (W1): draft book (W1)
  │ │
  │ o  A (W1): draft
  ├─╯
  @  base: public
  

Switch back to W2 and check that the pulled commit is there.
  $ hg cloud join -w w2 --switch -q
  $ showgraph
  o  S: public  remote/stable
  │
  │ o  M: public  remote/master
  ├─╯
  │ o  D (W2): draft book (W2)
  │ │
  │ o  C (W2): draft
  ├─╯
  │ o  A (W1): draft
  ├─╯
  @  base: public
  
 

Test switch to non default non-existent workspace
  $ hg cloud join --switch -w brand_new_empty
  abort: this repository can not be switched to the 'user/test/brand_new_empty' workspace
  the workspace doesn't exist (please use --create option to create the workspace)
  [255]
  $ hg cloud join --switch -w brand_new_empty --create
  commitcloud: synchronizing 'server' with 'user/test/w2'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/w2' to the 'user/test/brand_new_empty' workspace
  commitcloud: this repository is now connected to the 'user/test/brand_new_empty' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/brand_new_empty'
  commitcloud: commits synchronized
  finished in * (glob)
  $ showgraph
  o  M: public  remote/master
  │
  @  base: public
  
 

Test switch back to the default workpsace using a shorter `switch` command
  $ hg cloud switch -w default
  commitcloud: synchronizing 'server' with 'user/test/brand_new_empty'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/brand_new_empty' to the 'user/test/default' workspace
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)


Switch to another brand new workspace
  $ hg cloud switch -w brand_new_empty2 --create -q

Leave commit cloud
  $ hg cloud leave
  commitcloud: this repository is now disconnected from the 'user/test/brand_new_empty2' workspace

Try to switch without joining to any workspace first
  $ mkcommit "new"
  $ hg cloud switch -w default
  commitcloud: this repository can not be switched to the 'user/test/default' workspace
  the repository is not connected to any workspace yet and contains local commits or bookmarks

Clean the repository and try again. Disconnected but clean repo should be allowed to switch.
  $ showgraph
  @  new: draft
  │
  │ o  M: public  remote/master
  ├─╯
  o  base: public
  
  $ hg hide -r 'draft()'
  hiding commit 9adefff464dc "new"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at d20a80d4def3
  1 changeset hidden
  $ hg cloud switch -w default
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
