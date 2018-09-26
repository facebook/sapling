Demonstrates the "cannot rebase public commits" issue seen using hgsql and
pushrebase.

  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig hgsql.verbose=True
  $ enable strip
  $ setconfig ui.ssh='python "$RUNTESTDIR/dummyssh"'
  $ commit() {
  >   hg commit -Aq -m "$@"
  > }

  $ log() {
  >   hg log -G -T "{desc} [{phase}:{node|short}] {bookmarks} {remotenames}" "$@"
  > }

  $ config() {
  >   setconfig experimental.bundle2lazylocking=True
  >   setconfig pushrebase.runhgsqlsync=True
  >   enable pushrebase
  > }

  $ config_server() {
  >   config
  >   configureserver . foo
  > }

  $ clone() { # Usage: "clone <client directory> <source directory>
  >   SRC=${2:-server1}
  >   hg clone ssh://user@dummy/$SRC $1 -q
  >   cd $1
  >   hg up -q master
  >   config
  > }

Set up server repository.

  $ newrepo server1
  $ config_server
  $ echo foo > base
  $ commit "base"
  [hgsql] got lock after * seconds (glob)
  [hgsql] held lock for * seconds (glob)
  $ cd ..

Add a new commit to server2 (as if we stripped in server1):

  $ cp -R server1 server2
  $ cd server2
  $ echo foo > a
  $ commit "first"
  [hgsql] got lock after * seconds (glob)
  [hgsql] held lock for * seconds (glob)
  $ hg book -r . master
  [hgsql] got lock after * seconds (glob)
  [hgsql] held lock for * seconds (glob)
  $ log
  @  first [draft:8585ef078134] master
  |
  o  base [draft:4ced94c0a443]
  
Stop syncs in server1 so it doesn't pick up the new commit:
  $ cd ../server1
  $ cd .hg/store
  $ ln -s "foo:9" synclimiter
  $ cd ../../../

Clone client1 from the server2 repo (with the extra commit).

  $ clone client1 server2
  $ cd ../client1

Create a _third_ draft commit, push to the (behind) server1:

  $ echo "foo" > foo
  $ commit "third commit"
  $ rm ../server1/.hg/store/synclimiter_
  rm: cannot remove '../server1/.hg/store/synclimiter_': $ENOENT$
  [1]
  $ hg push --to master ssh://user@dummy/server1
  pushing to ssh://user@dummy/server1
  remote: [hgsql] skipping database sync because another process is already syncing
  searching for changes
  abort: cannot rebase public changesets: 8585ef078134
  [255]


  $ log
  @  third commit [draft:87df66bba286] master
  |
  o  first [public:8585ef078134]
  |
  o  base [public:4ced94c0a443]
  
  $ cd ../server1
  $ log
  [hgsql] skipping database sync because another process is already syncing
  @  base [draft:4ced94c0a443]
  
  $ cd ../server2
  $ touch .hg/flag
  $ wait

  $ log
  @  first [draft:8585ef078134] master
  |
  o  base [draft:4ced94c0a443]
  
