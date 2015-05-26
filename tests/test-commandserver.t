#if windows
  $ PYTHONPATH="$TESTDIR/../contrib;$PYTHONPATH"
#else
  $ PYTHONPATH="$TESTDIR/../contrib:$PYTHONPATH"
#endif
  $ export PYTHONPATH

typical client does not want echo-back messages, so test without it:

  $ grep -v '^promptecho ' < $HGRCPATH >> $HGRCPATH.new
  $ mv $HGRCPATH.new $HGRCPATH

  $ hg init repo
  $ cd repo

  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def hellomessage(server):
  ...     ch, data = readchannel(server)
  ...     print '%c, %r' % (ch, data)
  ...     # run an arbitrary command to make sure the next thing the server
  ...     # sends isn't part of the hello message
  ...     runcommand(server, ['id'])
  o, 'capabilities: getencoding runcommand\nencoding: *\npid: *' (glob)
  *** runcommand id
  000000000000 tip

  >>> from hgclient import check
  >>> @check
  ... def unknowncommand(server):
  ...     server.stdin.write('unknowncommand\n')
  abort: unknown command unknowncommand

  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def checkruncommand(server):
  ...     # hello block
  ...     readchannel(server)
  ... 
  ...     # no args
  ...     runcommand(server, [])
  ... 
  ...     # global options
  ...     runcommand(server, ['id', '--quiet'])
  ... 
  ...     # make sure global options don't stick through requests
  ...     runcommand(server, ['id'])
  ... 
  ...     # --config
  ...     runcommand(server, ['id', '--config', 'ui.quiet=True'])
  ... 
  ...     # make sure --config doesn't stick
  ...     runcommand(server, ['id'])
  ... 
  ...     # negative return code should be masked
  ...     runcommand(server, ['id', '-runknown'])
  *** runcommand 
  Mercurial Distributed SCM
  
  basic commands:
  
   add           add the specified files on the next commit
   annotate      show changeset information by line for each file
   clone         make a copy of an existing repository
   commit        commit the specified files or all outstanding changes
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   init          create a new repository in the given directory
   log           show revision history of entire repository or files
   merge         merge another revision into working directory
   pull          pull changes from the specified source
   push          push changes to the specified destination
   remove        remove the specified files on the next commit
   serve         start stand-alone webserver
   status        show changed files in the working directory
   summary       summarize working directory state
   update        update working directory (or switch revisions)
  
  (use "hg help" for the full list of commands or "hg -v" for details)
  *** runcommand id --quiet
  000000000000
  *** runcommand id
  000000000000 tip
  *** runcommand id --config ui.quiet=True
  000000000000
  *** runcommand id
  000000000000 tip
  *** runcommand id -runknown
  abort: unknown revision 'unknown'!
   [255]

  >>> from hgclient import readchannel, check
  >>> @check
  ... def inputeof(server):
  ...     readchannel(server)
  ...     server.stdin.write('runcommand\n')
  ...     # close stdin while server is waiting for input
  ...     server.stdin.close()
  ... 
  ...     # server exits with 1 if the pipe closed while reading the command
  ...     print 'server exit code =', server.wait()
  server exit code = 1

  >>> import cStringIO
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def serverinput(server):
  ...     readchannel(server)
  ... 
  ...     patch = """
  ... # HG changeset patch
  ... # User test
  ... # Date 0 0
  ... # Node ID c103a3dec114d882c98382d684d8af798d09d857
  ... # Parent  0000000000000000000000000000000000000000
  ... 1
  ... 
  ... diff -r 000000000000 -r c103a3dec114 a
  ... --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  ... +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  ... @@ -0,0 +1,1 @@
  ... +1
  ... """
  ... 
  ...     runcommand(server, ['import', '-'], input=cStringIO.StringIO(patch))
  ...     runcommand(server, ['log'])
  *** runcommand import -
  applying patch from stdin
  *** runcommand log
  changeset:   0:eff892de26ec
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  

check that --cwd doesn't persist between requests:

  $ mkdir foo
  $ touch foo/bar
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def cwd(server):
  ...     readchannel(server)
  ...     runcommand(server, ['--cwd', 'foo', 'st', 'bar'])
  ...     runcommand(server, ['st', 'foo/bar'])
  *** runcommand --cwd foo st bar
  ? bar
  *** runcommand st foo/bar
  ? foo/bar

  $ rm foo/bar


check that local configs for the cached repo aren't inherited when -R is used:

  $ cat <<EOF >> .hg/hgrc
  > [ui]
  > foo = bar
  > EOF

  >>> from hgclient import readchannel, sep, runcommand, check
  >>> @check
  ... def localhgrc(server):
  ...     readchannel(server)
  ... 
  ...     # the cached repo local hgrc contains ui.foo=bar, so showconfig should
  ...     # show it
  ...     runcommand(server, ['showconfig'], outfilter=sep)
  ... 
  ...     # but not for this repo
  ...     runcommand(server, ['init', 'foo'])
  ...     runcommand(server, ['-R', 'foo', 'showconfig', 'ui', 'defaults'])
  *** runcommand showconfig
  bundle.mainreporoot=$TESTTMP/repo
  defaults.backout=-d "0 0"
  defaults.commit=-d "0 0"
  defaults.shelve=--date "0 0"
  defaults.tag=-d "0 0"
  devel.all-warnings=true
  largefiles.usercache=$TESTTMP/.cache/largefiles
  ui.slash=True
  ui.interactive=False
  ui.mergemarkers=detailed
  ui.foo=bar
  ui.nontty=true
  *** runcommand init foo
  *** runcommand -R foo showconfig ui defaults
  defaults.backout=-d "0 0"
  defaults.commit=-d "0 0"
  defaults.shelve=--date "0 0"
  defaults.tag=-d "0 0"
  ui.slash=True
  ui.interactive=False
  ui.mergemarkers=detailed
  ui.nontty=true

  $ rm -R foo

#if windows
  $ PYTHONPATH="$TESTTMP/repo;$PYTHONPATH"
#else
  $ PYTHONPATH="$TESTTMP/repo:$PYTHONPATH"
#endif

  $ cat <<EOF > hook.py
  > import sys
  > def hook(**args):
  >     print 'hook talking'
  >     print 'now try to read something: %r' % sys.stdin.read()
  > EOF

  >>> import cStringIO
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def hookoutput(server):
  ...     readchannel(server)
  ...     runcommand(server, ['--config',
  ...                         'hooks.pre-identify=python:hook.hook',
  ...                         'id'],
  ...                input=cStringIO.StringIO('some input'))
  *** runcommand --config hooks.pre-identify=python:hook.hook id
  hook talking
  now try to read something: 'some input'
  eff892de26ec tip

  $ rm hook.py*

  $ echo a >> a
  >>> import os
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def outsidechanges(server):
  ...     readchannel(server)
  ...     runcommand(server, ['status'])
  ...     os.system('hg ci -Am2')
  ...     runcommand(server, ['tip'])
  ...     runcommand(server, ['status'])
  *** runcommand status
  M a
  *** runcommand tip
  changeset:   1:d3a0a68be6de
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  *** runcommand status

  >>> import os
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def bookmarks(server):
  ...     readchannel(server)
  ...     runcommand(server, ['bookmarks'])
  ... 
  ...     # changes .hg/bookmarks
  ...     os.system('hg bookmark -i bm1')
  ...     os.system('hg bookmark -i bm2')
  ...     runcommand(server, ['bookmarks'])
  ... 
  ...     # changes .hg/bookmarks.current
  ...     os.system('hg upd bm1 -q')
  ...     runcommand(server, ['bookmarks'])
  ... 
  ...     runcommand(server, ['bookmarks', 'bm3'])
  ...     f = open('a', 'ab')
  ...     f.write('a\n')
  ...     f.close()
  ...     runcommand(server, ['commit', '-Amm'])
  ...     runcommand(server, ['bookmarks'])
  *** runcommand bookmarks
  no bookmarks set
  *** runcommand bookmarks
     bm1                       1:d3a0a68be6de
     bm2                       1:d3a0a68be6de
  *** runcommand bookmarks
   * bm1                       1:d3a0a68be6de
     bm2                       1:d3a0a68be6de
  *** runcommand bookmarks bm3
  *** runcommand commit -Amm
  *** runcommand bookmarks
     bm1                       1:d3a0a68be6de
     bm2                       1:d3a0a68be6de
   * bm3                       2:aef17e88f5f0

  >>> import os
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def tagscache(server):
  ...     readchannel(server)
  ...     runcommand(server, ['id', '-t', '-r', '0'])
  ...     os.system('hg tag -r 0 foo')
  ...     runcommand(server, ['id', '-t', '-r', '0'])
  *** runcommand id -t -r 0
  
  *** runcommand id -t -r 0
  foo

  >>> import os
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def setphase(server):
  ...     readchannel(server)
  ...     runcommand(server, ['phase', '-r', '.'])
  ...     os.system('hg phase -r . -p')
  ...     runcommand(server, ['phase', '-r', '.'])
  *** runcommand phase -r .
  3: draft
  *** runcommand phase -r .
  3: public

  $ echo a >> a
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def rollback(server):
  ...     readchannel(server)
  ...     runcommand(server, ['phase', '-r', '.', '-p'])
  ...     runcommand(server, ['commit', '-Am.'])
  ...     runcommand(server, ['rollback'])
  ...     runcommand(server, ['phase', '-r', '.'])
  *** runcommand phase -r . -p
  no phases changed
   [1]
  *** runcommand commit -Am.
  *** runcommand rollback
  repository tip rolled back to revision 3 (undo commit)
  working directory now based on revision 3
  *** runcommand phase -r .
  3: public

  >>> import os
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def branch(server):
  ...     readchannel(server)
  ...     runcommand(server, ['branch'])
  ...     os.system('hg branch foo')
  ...     runcommand(server, ['branch'])
  ...     os.system('hg branch default')
  *** runcommand branch
  default
  marked working directory as branch foo
  (branches are permanent and global, did you want a bookmark?)
  *** runcommand branch
  foo
  marked working directory as branch default
  (branches are permanent and global, did you want a bookmark?)

  $ touch .hgignore
  >>> import os
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def hgignore(server):
  ...     readchannel(server)
  ...     runcommand(server, ['commit', '-Am.'])
  ...     f = open('ignored-file', 'ab')
  ...     f.write('')
  ...     f.close()
  ...     f = open('.hgignore', 'ab')
  ...     f.write('ignored-file')
  ...     f.close()
  ...     runcommand(server, ['status', '-i', '-u'])
  *** runcommand commit -Am.
  adding .hgignore
  *** runcommand status -i -u
  I ignored-file

  >>> import os
  >>> from hgclient import readchannel, sep, runcommand, check
  >>> @check
  ... def phasecacheafterstrip(server):
  ...     readchannel(server)
  ... 
  ...     # create new head, 5:731265503d86
  ...     runcommand(server, ['update', '-C', '0'])
  ...     f = open('a', 'ab')
  ...     f.write('a\n')
  ...     f.close()
  ...     runcommand(server, ['commit', '-Am.', 'a'])
  ...     runcommand(server, ['log', '-Gq'])
  ... 
  ...     # make it public; draft marker moves to 4:7966c8e3734d
  ...     runcommand(server, ['phase', '-p', '.'])
  ...     # load _phasecache.phaseroots
  ...     runcommand(server, ['phase', '.'], outfilter=sep)
  ... 
  ...     # strip 1::4 outside server
  ...     os.system('hg -q --config extensions.mq= strip 1')
  ... 
  ...     # shouldn't raise "7966c8e3734d: no node!"
  ...     runcommand(server, ['branches'])
  *** runcommand update -C 0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (leaving bookmark bm3)
  *** runcommand commit -Am. a
  created new head
  *** runcommand log -Gq
  @  5:731265503d86
  |
  | o  4:7966c8e3734d
  | |
  | o  3:b9b85890c400
  | |
  | o  2:aef17e88f5f0
  | |
  | o  1:d3a0a68be6de
  |/
  o  0:eff892de26ec
  
  *** runcommand phase -p .
  *** runcommand phase .
  5: public
  *** runcommand branches
  default                        1:731265503d86

  $ cat >> .hg/hgrc << EOF
  > [experimental]
  > evolution=createmarkers
  > EOF

  >>> import os
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def obsolete(server):
  ...     readchannel(server)
  ... 
  ...     runcommand(server, ['up', 'null'])
  ...     runcommand(server, ['phase', '-df', 'tip'])
  ...     cmd = 'hg debugobsolete `hg log -r tip --template {node}`'
  ...     if os.name == 'nt':
  ...         cmd = 'sh -c "%s"' % cmd # run in sh, not cmd.exe
  ...     os.system(cmd)
  ...     runcommand(server, ['log', '--hidden'])
  ...     runcommand(server, ['log'])
  *** runcommand up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  *** runcommand phase -df tip
  *** runcommand log --hidden
  changeset:   1:731265503d86
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     .
  
  changeset:   0:eff892de26ec
  bookmark:    bm1
  bookmark:    bm2
  bookmark:    bm3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  *** runcommand log
  changeset:   0:eff892de26ec
  bookmark:    bm1
  bookmark:    bm2
  bookmark:    bm3
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  

  $ cat <<EOF >> .hg/hgrc
  > [extensions]
  > mq =
  > EOF

  >>> import os
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def mqoutsidechanges(server):
  ...     readchannel(server)
  ... 
  ...     # load repo.mq
  ...     runcommand(server, ['qapplied'])
  ...     os.system('hg qnew 0.diff')
  ...     # repo.mq should be invalidated
  ...     runcommand(server, ['qapplied'])
  ... 
  ...     runcommand(server, ['qpop', '--all'])
  ...     os.system('hg qqueue --create foo')
  ...     # repo.mq should be recreated to point to new queue
  ...     runcommand(server, ['qqueue', '--active'])
  *** runcommand qapplied
  *** runcommand qapplied
  0.diff
  *** runcommand qpop --all
  popping 0.diff
  patch queue now empty
  *** runcommand qqueue --active
  foo

  $ cat <<EOF > dbgui.py
  > import os, sys
  > from mercurial import cmdutil, commands
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command("debuggetpass", norepo=True)
  > def debuggetpass(ui):
  >     ui.write("%s\\n" % ui.getpass())
  > @command("debugprompt", norepo=True)
  > def debugprompt(ui):
  >     ui.write("%s\\n" % ui.prompt("prompt:"))
  > @command("debugreadstdin", norepo=True)
  > def debugreadstdin(ui):
  >     ui.write("read: %r\n" % sys.stdin.read(1))
  > @command("debugwritestdout", norepo=True)
  > def debugwritestdout(ui):
  >     os.write(1, "low-level stdout fd and\n")
  >     sys.stdout.write("stdout should be redirected to /dev/null\n")
  >     sys.stdout.flush()
  > EOF
  $ cat <<EOF >> .hg/hgrc
  > [extensions]
  > dbgui = dbgui.py
  > EOF

  >>> import cStringIO
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def getpass(server):
  ...     readchannel(server)
  ...     runcommand(server, ['debuggetpass', '--config',
  ...                         'ui.interactive=True'],
  ...                input=cStringIO.StringIO('1234\n'))
  ...     runcommand(server, ['debugprompt', '--config',
  ...                         'ui.interactive=True'],
  ...                input=cStringIO.StringIO('5678\n'))
  ...     runcommand(server, ['debugreadstdin'])
  ...     runcommand(server, ['debugwritestdout'])
  *** runcommand debuggetpass --config ui.interactive=True
  password: 1234
  *** runcommand debugprompt --config ui.interactive=True
  prompt: 5678
  *** runcommand debugreadstdin
  read: ''
  *** runcommand debugwritestdout


run commandserver in commandserver, which is silly but should work:

  >>> import cStringIO
  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def nested(server):
  ...     print '%c, %r' % readchannel(server)
  ...     class nestedserver(object):
  ...         stdin = cStringIO.StringIO('getencoding\n')
  ...         stdout = cStringIO.StringIO()
  ...     runcommand(server, ['serve', '--cmdserver', 'pipe'],
  ...                output=nestedserver.stdout, input=nestedserver.stdin)
  ...     nestedserver.stdout.seek(0)
  ...     print '%c, %r' % readchannel(nestedserver)  # hello
  ...     print '%c, %r' % readchannel(nestedserver)  # getencoding
  o, 'capabilities: getencoding runcommand\nencoding: *\npid: *' (glob)
  *** runcommand serve --cmdserver pipe
  o, 'capabilities: getencoding runcommand\nencoding: *\npid: *' (glob)
  r, '*' (glob)


start without repository:

  $ cd ..

  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def hellomessage(server):
  ...     ch, data = readchannel(server)
  ...     print '%c, %r' % (ch, data)
  ...     # run an arbitrary command to make sure the next thing the server
  ...     # sends isn't part of the hello message
  ...     runcommand(server, ['id'])
  o, 'capabilities: getencoding runcommand\nencoding: *\npid: *' (glob)
  *** runcommand id
  abort: there is no Mercurial repository here (.hg not found)
   [255]

  >>> from hgclient import readchannel, runcommand, check
  >>> @check
  ... def startwithoutrepo(server):
  ...     readchannel(server)
  ...     runcommand(server, ['init', 'repo2'])
  ...     runcommand(server, ['id', '-R', 'repo2'])
  *** runcommand init repo2
  *** runcommand id -R repo2
  000000000000 tip


unix domain socket:

  $ cd repo
  $ hg update -q

#if unix-socket unix-permissions

  >>> import cStringIO
  >>> from hgclient import unixserver, readchannel, runcommand, check
  >>> server = unixserver('.hg/server.sock', '.hg/server.log')
  >>> def hellomessage(conn):
  ...     ch, data = readchannel(conn)
  ...     print '%c, %r' % (ch, data)
  ...     runcommand(conn, ['id'])
  >>> check(hellomessage, server.connect)
  o, 'capabilities: getencoding runcommand\nencoding: *\npid: *' (glob)
  *** runcommand id
  eff892de26ec tip bm1/bm2/bm3
  >>> def unknowncommand(conn):
  ...     readchannel(conn)
  ...     conn.stdin.write('unknowncommand\n')
  >>> check(unknowncommand, server.connect)  # error sent to server.log
  >>> def serverinput(conn):
  ...     readchannel(conn)
  ...     patch = """
  ... # HG changeset patch
  ... # User test
  ... # Date 0 0
  ... 2
  ... 
  ... diff -r eff892de26ec -r 1ed24be7e7a0 a
  ... --- a/a
  ... +++ b/a
  ... @@ -1,1 +1,2 @@
  ...  1
  ... +2
  ... """
  ...     runcommand(conn, ['import', '-'], input=cStringIO.StringIO(patch))
  ...     runcommand(conn, ['log', '-rtip', '-q'])
  >>> check(serverinput, server.connect)
  *** runcommand import -
  applying patch from stdin
  *** runcommand log -rtip -q
  2:1ed24be7e7a0
  >>> server.shutdown()

  $ cat .hg/server.log
  listening at .hg/server.sock
  abort: unknown command unknowncommand
  killed!
#endif
#if no-unix-socket

  $ hg serve --cmdserver unix -a .hg/server.sock
  abort: unsupported platform
  [255]

#endif
