#inprocess-hg-incompatible
  $ setconfig devel.legacy.exchange=bookmarks

Setup

  $ configure dummyssh
We are testing legacy pushrebase - we need legacy hg server.
  $ rm $TESTTMP/.eagerepo
  $ enable pushrebase

  $ cat >> "$TESTTMP/hook.py" << EOF
  > import bindings
  > from binascii import unhexlify as bin
  > def log(repo, namespace, key, old, new, **kwargs):
  >   io = bindings.io.IO.main()
  >   commits = repo.commits()
  >   def desc(hex_node):
  >     return commits.getcommitfields(bin(hex_node)).description()
  >   io.write_err(("log %s %s %s -> %s\n" % (namespace, key, desc(old), desc(new))).encode())
  >   return True # True means block the commit here
  > EOF

Set up server repository

  $ newserver server
  $ setconfig pushrebase.blocknonpushrebase=False
  $ setconfig hooks.prepushkey.log="python:$TESTTMP/hook.py:log"
  $ drawdag << 'EOF'
  > commitA
  > EOF
  $ sl bookmark -r "$commitA" master

Set up client repository

  $ cd "$TESTTMP"
  $ clone server client
  $ cd client
  $ sl up master -q

Set up the client to commit on the server-side when a push happens. This simulates a race.

  $ cd "$TESTTMP/client"
  $ setconfig extensions.pushrebase=

  $ cat >> $TESTTMP/wrapper.py << 'EOF'
  > import os
  > from sapling import exchange, extensions
  > def wrapper(orig, pushop):
  >   r = orig(pushop)
  >   testtmp = os.environ["TESTTMP"]
  >   server = os.path.join(testtmp, "server")
  >   sl = os.environ.get("HGEXECUTABLEPATH", "sl")
  >   script = "\n".join([
  >     "cd " + server,
  >     sl + " up -q master",
  >     "touch X",
  >     sl + " debugdrawdag --cwd " + server + " << 'EOS'",
  >     "commitX",
  >     "|",
  >     "master",
  >     "EOS",
  >     sl + " bookmark -fr tip master",
  >     "echo committed",
  >     sl + " --config experimental.graph.renderer=ascii log -Gr 'all()' -T '{desc} {bookmark}'",
  >   ])
  >   pushop.repo.ui.pushbuffer(error=True, subproc=True)
  >   try:
  >     pushop.repo.ui.system(script)
  >     output = pushop.repo.ui.popbuffer()
  >   except Exception:
  >     pushop.repo.ui.popbuffer()
  >     raise
  >   pushop.repo.ui.write(output)
  >   return r
  > def extsetup(ui):
  >   extensions.wrapfunction(exchange, '_pushdiscovery', wrapper)
  > EOF

Create commit for the client

Push without pushrebase, and check that the hook sees the commit that was actually pushed off of (commitA).

  $ cd "$TESTTMP/client"
  $ setconfig extensions.pushrebase=!

  $ touch C
  $ sl commit -Aqm 'commitC'
  $ sl log -Gr 'all()' -T '{desc} {bookmark}'
  @  commitC
  │
  o  commitA
  
  $ sl push --to master --force --config extensions.wrapper="$TESTTMP/wrapper.py"
  pushing rev 24c8a95a9829 to destination ssh://user@dummy/server bookmark master
  searching for changes
  committed
  o  commitX
  |
  @  commitA
  
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: log bookmarks master commitA -> commitC
  remote: pushkey-abort: prepushkey.log hook failed
  remote: transaction abort! (?)
  remote: rollback completed (?)
  abort: updating bookmark master failed!
  [255]

Now, do a pushrebase. First, reset the server-side.

  $ cd "$TESTTMP/server"
  $ sl bookmark -f -r "$commitA" master

Then, pushrebase. This time, we expect the pushkey to be updated

  $ cd "$TESTTMP/client"
  $ setconfig extensions.pushrebase=

  $ sl push --to master --config extensions.wrapper="$TESTTMP/wrapper.py"
  pushing rev 24c8a95a9829 to destination ssh://user@dummy/server bookmark master
  searching for changes
  committed
  o  commitX
  |
  @  commitA
  
  remote: pushing 1 changeset:
  remote:     24c8a95a9829  commitC
  remote: 2 new changesets from the server will be downloaded
  remote: log bookmarks master commitX -> commitC
  remote: pushkey-abort: prepushkey.log hook failed
  remote: transaction abort! (?)
  remote: rollback completed (?)
  abort: updating bookmark master failed!
  [255]
