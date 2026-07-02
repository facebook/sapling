
#require no-eden


Create a repository:

  $ newclientrepo t

Prepare a changeset:

  $ echo a > a
  $ sl add a

  $ sl status
  A a

Writes to stdio succeed and fail appropriately

#if devfull
  $ sl status 2>/dev/full
  A a

  $ sl status >/dev/full
  abort: error flushing command output: No space left on device (os error 28)
  [255]
#endif

#if bash
Commands can succeed without a stdin
  $ sl log -r tip 0<&-
  commit:      000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  
#endif

#if devfull
  $ sl status >/dev/full 2>&1
  [255]

  $ sl status ENOENT 2>/dev/full
  [255]
#endif

  $ sl commit -m test

This command is ancient:

  $ sl history
  commit:      acb14030fe0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  

Verify that updating to revision acb14030fe0a via commands.update() works properly

  $ cat <<EOF > update_to_rev0.py
  > from sapling import ui, hg, commands
  > myui = ui.ui.load()
  > repo = hg.repository(myui, path='.')
  > commands.update(myui, repo, rev='acb14030fe0a')
  > EOF
  $ sl up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl debugshell ./update_to_rev0.py
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl identify -n
  72057594037927936


Poke around at hashes:

  $ sl manifest --debug
  b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3 644   a

  $ sl cat a
  a

Verify should succeed:

  $ sl verify
  commit graph passed quick local checks
  (pass --dag to perform slow checks with server)

At the end...

  $ cd ..
