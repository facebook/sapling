  $ disable treemanifest
  $ setconfig devel.legacy.exchange=bookmarks

Setup

  $ configure dummyssh
  $ enable pushrebase remotenames

  $ cat >> "$TESTTMP/commit.sh" << EOF
  > #!/bin/bash
  > cd "$TESTTMP/server"
  > hg up -q master
  > touch X
  > hg debugdrawdag --cwd "$TESTTMP/server" << EOS
  > commitX
  > |
  > master
  > EOS
  > hg bookmark -fr tip master
  > echo committed
  > hg log -Gr 'all()' -T '{desc} {bookmark}'
  > EOF

  $ cat >> "$TESTTMP/hook.py" << EOF
  > def log(ui, repo, namespace, key, old, new, **kwargs):
  >   ui.write_err("log %s %s %s -> %s\n" % (namespace, key, repo[old].description(), repo[new].description()))
  >   return True # True means block the commit here
  > EOF

Set up server repository

  $ newserver server
  $ setconfig pushrebase.blocknonpushrebase=False
  $ setconfig hooks.prepushkey.log="python:$TESTTMP/hook.py:log"
  $ drawdag << 'EOF'
  > commitA
  > EOF
  $ hg bookmark -r "$commitA" master

Set up client repository

  $ cd "$TESTTMP"
  $ clone server client
  $ cd client
  $ hg up master -q

Set up the client to commit on the server-side when a push happens. This simulates a race.

  $ cd "$TESTTMP/client"
  $ set config extensions.pushrebase=

  $ cat >> $TESTTMP/wrapper.py << EOF
  > from edenscm.mercurial import exchange, extensions
  > def wrapper(orig, pushop):
  >   r = orig(pushop)
  >   pushop.repo.ui.system("bash $TESTTMP/commit.sh")
  >   return r
  > def extsetup(ui):
  >   extensions.wrapfunction(exchange, '_pushdiscovery', wrapper)
  > EOF

Create commit for the client

Push without pushrebase, and check that the hook sees the commit that was actually pushed off of (commitA).

  $ cd "$TESTTMP/client"
  $ setconfig extensions.pushrebase=!

  $ touch C
  $ hg commit -Aqm 'commitC'
  $ hg log -Gr 'all()' -T '{desc} {bookmark}'
  @  commitC
  |
  o  commitA
  
  $ hg push --to master --force --config extensions.wrapper="$TESTTMP/wrapper.py"
  pushing rev 24c8a95a9829 to destination ssh://user@dummy/server bookmark master
  searching for changes
  committed
  o  commitX
  |
  @  commitA
  
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: log bookmarks master commitA -> commitC
  remote: pushkey-abort: prepushkey.log hook failed
  remote: transaction abort!
  remote: rollback completed
  abort: updating bookmark master failed!
  [255]

Now, do a pushrebase. First, reset the server-side.

  $ cd "$TESTTMP/server"
  $ hg bookmark -f -r "$commitA" master

Then, pushrebase. This time, we expect the pushkey to be updated

  $ cd "$TESTTMP/client"
  $ setconfig extensions.pushrebase=

  $ hg push --to master --config extensions.wrapper="$TESTTMP/wrapper.py"
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
  remote: transaction abort!
  remote: rollback completed
  abort: updating bookmark master failed!
  [255]
