#chg-compatible

Demonstrates the "cannot rebase public commits" issue seen using hgsql and
pushrebase.

  $ configure dummyssh
  $ disable treemanifest
  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig hgsql.verbose=True
  $ setconfig pushrebase.verbose=True
  $ enable pushrebase
  $ enable strip
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
  [hgsql] got lock after * seconds (read 1 rows) (glob)
  [hgsql] held lock for * seconds (read 5 rows; write 5 rows) (glob)
  $ cd ..

Add a new commit to server2 (as if we stripped in server1):

  $ cp -R server1 server2
  $ cd server2
  $ echo foo > a
  $ commit "first"
  [hgsql] got lock after * seconds (read 1 rows) (glob)
  [hgsql] held lock for * seconds (read 8 rows; write 7 rows) (glob)
  $ hg book -r . master
  [hgsql] got lock after * seconds (read 1 rows) (glob)
  [hgsql] held lock for * seconds (read 5 rows; write 1 rows) (glob)
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
  remote: [hgsql] getting 1 commits from database
  searching for changes
  remote: checking conflicts with 8585ef078134
  remote: pushing 1 changeset:
  remote:     87df66bba286  third commit
  remote: [hgsql] got lock after * seconds (read 1 rows) (glob)
  remote: rebasing stack from 8585ef078134 onto 8585ef078134
  remote: [hgsql] held lock for * seconds (read 8 rows; write 9 rows) (glob)
  updating bookmark master


  $ log
  @  third commit [public:87df66bba286] master
  |
  o  first [public:8585ef078134]
  |
  o  base [public:4ced94c0a443]
  
  $ cd ../server1
  $ log
  [hgsql] skipping database sync because another process is already syncing
  o  third commit [public:87df66bba286] master
  |
  o  first [public:8585ef078134]
  |
  @  base [public:4ced94c0a443]
  
  $ cd ../server2
  $ log
  [hgsql] getting 1 commits from database
  o  third commit [draft:87df66bba286] master
  |
  @  first [draft:8585ef078134]
  |
  o  base [draft:4ced94c0a443]
  
