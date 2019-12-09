#chg-compatible

  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig extensions.treemanifest=!

# Populate the db with an initial commit

  $ initclient client
  $ cd client
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ initserver master masterrepo
  $ cd master
  $ printf '[phases]\npublish=True\n' >> .hg/hgrc
  $ hg log
  $ hg pull -q ../client

  $ cd ..

# Verify local pushes work

  $ cd client
  $ echo y > y
  $ hg commit -qAm y
  $ hg phase -p -r 'all()'
  $ hg push ../master --traceback
  pushing to ../master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

# Verify local pulls work
  $ hg debugstrip -q -r tip
  $ hg pull ../master
  pulling from ../master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets d34c38483be9
  $ hg log --template '{rev} {desc}\n'
  1 y
  0 x

# Verify local bookmark pull

  $ cd ../master
  $ hg book foo -r 0
  $ hg book
     foo                       0:b292c1e3311f
  $ cd ../client
  $ hg pull -q ../master
  $ hg book
     foo                       0:b292c1e3311f

# Verify local bookmark push

  $ hg book -r tip foo
  moving bookmark 'foo' forward from b292c1e3311f
  $ hg push ../master
  pushing to ../master
  searching for changes
  no changes found
  updating bookmark foo
  [1]
  $ hg book -R ../master
     foo                       1:d34c38483be9

# Verify explicit bookmark pulls work

  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo z > z
  $ hg commit -qAm z
  $ hg book foo
  moving bookmark 'foo' forward from d34c38483be9
  $ cd ../master
  $ hg pull -B foo ../client
  pulling from ../client
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating bookmark foo
  new changesets d47967ce72a5
  $ hg log -l 1 --template '{rev} {bookmarks}\n'
  2 foo

# Push from hgsql to other repo

  $ hg up -q tip
  $ echo zz > z
  $ hg commit -m z2
  $ hg push ../client
  pushing to ../client
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

# Verify that multiple heads and bookmarks work

  $ cd ../client
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (leaving bookmark foo)
  $ echo a > a
  $ hg commit -qAm a
  $ hg book bar
  $ hg push -f ../master -B bar
  pushing to ../master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  exporting bookmark bar
  $ hg log -R ../master -T '{rev} {bookmarks}\n' -G
  o  4 bar
  |
  | @  3
  | |
  | o  2 foo
  | |
  | o  1
  |/
  o  0
  
# Verify syncing with hg-ssh --readonly works
  $ cd ../
  $ cat > ssh.sh << EOF
  > userhost="\$1"
  > SSH_ORIGINAL_COMMAND="\$2"
  > export SSH_ORIGINAL_COMMAND
  > PYTHONPATH="$PYTHONPATH"
  > export PYTHONPATH
  > hg debugpython -- "$TESTDIR/../contrib/hg-ssh" --read-only "$TESTTMP/master"
  > EOF

  $ hg -R master --config hgsql.bypass=True debugstrip -r tip
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/2fbb8bb2b903-cf7ff44e-backup.hg (glob)
  $ hg -R client pull --ssh "sh ssh.sh" "ssh://user@dummy/$TESTTMP/master"
  pulling from ssh://user@dummy/$TESTTMP/master
  searching for changes
  no changes found

# Verify syncing with pretxnclose hooks works
  $ initserver master2 masterrepo
  $ cd master
  $ touch testpretxnclose
  $ hg commit -Aqm "test pretxnclose"
  $ cd ../master2
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > pretxnclose.abort=exit 1
  > EOF
  $ hg log -r tip -T '{rev}\n'
  5
  $ hg debugstrip -q -r tip --config hgsql.bypass=True --config hooks.pretxnclose.abort=

# Verify hooks still run, even after sync disabled them temporarily
  $ cd ../client
  $ hg pull -q ../master
  $ hg up -q tip
  $ echo x >> testpretxnclose
  $ hg commit -qm "test pretxnclose 2"
  $ hg push ../master2
  pushing to ../master2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  transaction abort!
  rollback completed
  abort: pretxnclose.abort hook exited with status 1
  [255]
