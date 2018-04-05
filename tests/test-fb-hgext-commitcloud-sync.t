  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend =
  > infinitepush =
  > commitcloud =
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > [infinitepush]
  > branchpattern = re:scratch/.*
  > [commitcloud]
  > hostname = testhost
  > [alias]
  > tglog = log -G --template "{node|short} '{desc}' {bookmarks}\n"
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF

  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg commit -Aqm "$1"
  > }

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > server = yes
  > indextype = disk
  > storetype = disk
  > reponame = testrepo
  > EOF

  $ mkcommit "base"
  $ cd ..

Make shared part of config
  $ cat >> shared.rc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > user_token_path = $TESTTMP
  > auth_help = visit htts://localhost/oauth to generate a registration token
  > owner_team = The Test Team @ FB
  > EOF

Make the first clone of the server
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ cat ../shared.rc >> .hg/hgrc
Joining before registration:
  $ hg cloudjoin
  abort: #commitcloud registration error: please run `hg cloudregister` before joining a workspace
  authentication instructions:
  visit htts://localhost/oauth to generate a registration token
  please contact The Test Team @ FB for more information
  [255]
Registration:
  $ hg cloudregister
  #commitcloud welcome to registration!
  abort: #commitcloud registration error: token is not provided and not found
  authentication instructions:
  visit htts://localhost/oauth to generate a registration token
  please contact The Test Team @ FB for more information
  [255]
  $ hg cloudregister -t xxxxxx
  #commitcloud welcome to registration!
  registration successful
  $ hg cloudregister -t xxxxxx --config "commitcloud.user_token_path=$TESTTMP/somedir"
  #commitcloud welcome to registration!
  abort: #commitcloud unexpected configuration error: invalid commitcloud.user_token_path '$TESTTMP/somedir'
  please contact The Test Team @ FB to report misconfiguration
  [255]
Joining:
  $ hg cloudsync
  abort: #commitcloud workspace error: undefined workspace
  your repo is not connected to any workspace
  please run `hg cloudjoin --help` for more details
  [255]
  $ hg cloudjoin
  #commitcloud this repository is now part of the 'user/test/default' workspace for the 'server' repo

  $ cd ..

Make the second clone of the server
  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ cat ../shared.rc >> .hg/hgrc
Registration:
  $ hg cloudregister
  #commitcloud welcome to registration!
  you have been already registered
  $ hg cloudregister -t yyyyy
  #commitcloud welcome to registration!
  your token will be updated
  registration successful
Joining:
  $ hg cloudjoin
  #commitcloud this repository is now part of the 'user/test/default' workspace for the 'server' repo

  $ cd ..

Make a commit in the first client, and sync it
  $ cd client1
  $ mkcommit "commit1"
  $ hg pushbackup -q
  $ hg cloudsync
  #commitcloud start synchronization
  #commitcloud cloudsync done
  $ cd ..

Sync from the second client - the commit should appear
  $ cd client2
  $ hg cloudsync
  #commitcloud start synchronization
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets fa5d62c46fd7
  (run 'hg update' to get a working copy)
  #commitcloud cloudsync done

  $ hg up -q tip
  $ hg tglog
  @  fa5d62c46fd7 'commit1'
  |
  o  d20a80d4def3 'base'
  
Make a commit from the second client and sync it
  $ mkcommit "commit2"
  $ hg pushbackup -q
  $ hg cloudsync
  #commitcloud start synchronization
  #commitcloud cloudsync done
  $ cd ..

On the first client, make a bookmark, then sync - the bookmark and new commit should be synced
  $ cd client1
  $ hg bookmark -r 0 bookmark1
  $ hg cloudsync
  #commitcloud start synchronization
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 2 files
  new changesets 02f6fc2b7154
  (run 'hg update' to get a working copy)
  #commitcloud cloudsync done
  $ hg tglog
  o  02f6fc2b7154 'commit2'
  |
  @  fa5d62c46fd7 'commit1'
  |
  o  d20a80d4def3 'base' bookmark1
  
  $ cd ..

Sync the bookmark back to the second client
  $ cd client2
  $ hg cloudsync
  #commitcloud start synchronization
  #commitcloud cloudsync done
  $ hg tglog
  @  02f6fc2b7154 'commit2'
  |
  o  fa5d62c46fd7 'commit1'
  |
  o  d20a80d4def3 'base' bookmark1
  
Move the bookmark on the second client, and then sync it
  $ hg bookmark -r 2 -f bookmark1
  $ hg cloudsync
  #commitcloud start synchronization
  #commitcloud cloudsync done
  $ cd ..

Move the bookmark also on the first client, it should be forked in the sync
  $ cd client1
  $ hg bookmark -r 1 -f bookmark1
  $ hg cloudsync
  #commitcloud start synchronization
  bookmark1 changed locally and remotely, local bookmark renamed to bookmark1-testhost
  #commitcloud cloudsync done
  $ hg tglog
  o  02f6fc2b7154 'commit2' bookmark1
  |
  @  fa5d62c46fd7 'commit1' bookmark1-testhost
  |
  o  d20a80d4def3 'base'
  
  $ cd ..

Amend a commit
  $ cd client1
  $ echo more >> commit1
  $ hg amend --rebase -m "commit1 amended"
  rebasing 2:02f6fc2b7154 "commit2" (bookmark1)
  $ hg pushbackup -q
  $ hg cloudsync
  #commitcloud start synchronization
  #commitcloud cloudsync done
  $ hg tglog
  o  48610b1a7ec0 'commit2' bookmark1
  |
  @  a7bb357e7299 'commit1 amended' bookmark1-testhost
  |
  o  d20a80d4def3 'base'
  
  $ cd ..

Sync the amended commit to the other client
  $ cd client2
  $ hg cloudsync
  #commitcloud start synchronization
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files (+1 heads)
  new changesets a7bb357e7299:48610b1a7ec0
  (run 'hg heads' to see heads, 'hg merge' to merge)
  #commitcloud cloudsync done
  $ hg up -q tip
  $ hg tglog
  @  48610b1a7ec0 'commit2' bookmark1
  |
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  |
  o  d20a80d4def3 'base'
  
  $ test ! -f .hg/store/commitcloudpendingobsmarkers

  $ cd ..

Test recovery from broken state (example: invalid json)
  $ cd client1
  $ echo '}}}' >> .hg/store/commitcloudstate.usertestdefault.b6eca
  $ hg cloudsync 2>&1
  abort: #commitcloud invalid workspace data: 'failed to parse commitcloudstate.usertestdefault.b6eca'
  please run `hg cloudrecover`
  [255]
  $ hg cloudrecover
  #commitcloud start recovering
  #commitcloud start synchronization
  #commitcloud cloudsync done

