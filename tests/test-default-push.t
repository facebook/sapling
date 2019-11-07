  $ setconfig extensions.treemanifest=!
  $ hg init a

  $ echo a > a/a
  $ hg --cwd a ci -Ama
  adding a

  $ hg clone a c
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo b >> b/a
  $ hg --cwd b ci -mb

Push should provide a hint when both 'default' and 'default-push' not set:
  $ cd c
  $ hg push --config paths.default=
  abort: default repository not configured!
  (see 'hg help config.paths')
  [255]

  $ cd ..

Push should push to 'default' when 'default-push' not set:

  $ hg --cwd b push
  pushing to $TESTTMP/a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Push should push to 'default-push' when set:

  $ echo '[paths]' >> b/.hg/hgrc
  $ echo 'default-push = ../c' >> b/.hg/hgrc
  $ hg --cwd b push
  pushing to $TESTTMP/c
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

But push should push to 'default' if explicitly specified (issue5000):

  $ hg --cwd b push default
  pushing to $TESTTMP/a
  searching for changes
  no changes found
  [1]

Push should push to 'default-push' when 'default' is not set

  $ hg -q clone a push-default-only
  $ cd push-default-only
  $ rm .hg/hgrc

  $ touch foo
  $ hg -q commit -A -m 'add foo'
  $ hg --config paths.default-push=../a push
  pushing to $TESTTMP/a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

  $ cd ..

Pushing to a path that isn't defined should not fall back to default

  $ hg --cwd b push doesnotexist
  abort: repository doesnotexist does not exist!
  [255]

:pushurl is used when defined

  $ hg -q clone a pushurlsource
  $ hg -q clone a pushurldest
  $ cd pushurlsource

Windows needs a leading slash to make a URL that passes all of the checks
  $ WD=`pwd`
#if windows
  $ WD="/$WD"
#endif
  $ cat > .hg/hgrc << EOF
  > [paths]
  > default = https://example.com/not/relevant
  > default:pushurl = file://$WD/../pushurldest
  > EOF

  $ touch pushurl
  $ hg -q commit -A -m 'add pushurl'
  $ hg push
  pushing to file:/*/$TESTTMP/pushurlsource/../pushurldest (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

:pushrev is used when no -r is passed

  $ cat >> .hg/hgrc << EOF
  > default:pushrev = .
  > EOF
  $ hg -q up -r 0
  $ echo head1 > foo
  $ hg -q commit -A -m head1
  $ hg -q up -r 0
  $ echo head2 > foo
  $ hg -q commit -A -m head2
  $ hg push -f
  pushing to file:/*/$TESTTMP/pushurlsource/../pushurldest (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

  $ hg --config 'paths.default:pushrev=draft()' push -f
  pushing to file:/*/$TESTTMP/pushurlsource/../pushurldest (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Invalid :pushrev raises appropriately

  $ hg --config 'paths.default:pushrev=notdefined()' push
  pushing to file:/*/$TESTTMP/pushurlsource/../pushurldest (glob)
  hg: parse error: unknown identifier: notdefined
  [255]

  $ hg --config 'paths.default:pushrev=(' push
  pushing to file:/*/$TESTTMP/pushurlsource/../pushurldest (glob)
  hg: parse error at 1: not a prefix: end
  ((
    ^ here)
  [255]

  $ cd ..
