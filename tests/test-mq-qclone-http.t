#require killdaemons

hide outer repo
  $ hg init

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ mkdir webdir
  $ cd webdir
  $ hg init a
  $ hg --cwd a qinit -c
  $ echo a > a/a
  $ hg --cwd a ci -A -m a
  adding a
  $ echo b > a/b
  $ hg --cwd a addremove
  adding b
  $ hg --cwd a qnew -f b.patch
  $ hg --cwd a qcommit -m b.patch
  $ hg --cwd a log --template "{desc}\n"
  [mq]: b.patch
  a
  $ hg --cwd a/.hg/patches log --template "{desc}\n"
  b.patch
  $ root=`pwd`
  $ cd ..

test with recursive collection

  $ cat > collections.conf <<EOF
  > [paths]
  > /=$root/**
  > EOF
  $ hg serve -p $HGPORT -d --pid-file=hg.pid --webdir-conf collections.conf \
  >     -A access-paths.log -E error-paths-1.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ get-with-headers.py localhost:$HGPORT '?style=raw'
  200 Script output follows
  
  
  /a/
  /a/.hg/patches/
  
  $ hg qclone http://localhost:$HGPORT/a b
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --cwd b log --template "{desc}\n"
  a
  $ hg --cwd b qpush -a
  applying b.patch
  now at: b.patch
  $ hg --cwd b log --template "{desc}\n"
  imported patch b.patch
  a

test with normal collection

  $ cat > collections1.conf <<EOF
  > [paths]
  > /=$root/*
  > EOF
  $ hg serve -p $HGPORT1 -d --pid-file=hg.pid --webdir-conf collections1.conf \
  >     -A access-paths.log -E error-paths-1.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ get-with-headers.py localhost:$HGPORT1 '?style=raw'
  200 Script output follows
  
  
  /a/
  /a/.hg/patches/
  
  $ hg qclone http://localhost:$HGPORT1/a c
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --cwd c log --template "{desc}\n"
  a
  $ hg --cwd c qpush -a
  applying b.patch
  now at: b.patch
  $ hg --cwd c log --template "{desc}\n"
  imported patch b.patch
  a

test with old-style collection

  $ cat > collections2.conf <<EOF
  > [collections]
  > $root=$root
  > EOF
  $ hg serve -p $HGPORT2 -d --pid-file=hg.pid --webdir-conf collections2.conf \
  >     -A access-paths.log -E error-paths-1.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ get-with-headers.py localhost:$HGPORT2 '?style=raw'
  200 Script output follows
  
  
  /a/
  /a/.hg/patches/
  
  $ hg qclone http://localhost:$HGPORT2/a d
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --cwd d log --template "{desc}\n"
  a
  $ hg --cwd d qpush -a
  applying b.patch
  now at: b.patch
  $ hg --cwd d log --template "{desc}\n"
  imported patch b.patch
  a

test --mq works and uses correct repository config

  $ hg --cwd d outgoing --mq
  comparing with http://localhost:$HGPORT2/a/.hg/patches
  searching for changes
  no changes found
  [1]
  $ hg --cwd d log --mq --template '{rev} {desc|firstline}\n'
  0 b.patch

  $ killdaemons.py $DAEMON_PIDS

