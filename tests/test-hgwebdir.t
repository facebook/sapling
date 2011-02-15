Tests some basic hgwebdir functionality. Tests setting up paths and
collection, different forms of 404s and the subdirectory support.

  $ mkdir webdir
  $ cd webdir
  $ hg init a
  $ echo a > a/a
  $ hg --cwd a ci -Ama -d'1 0'
  adding a

create a mercurial queue repository

  $ hg --cwd a qinit --config extensions.hgext.mq= -c
  $ hg init b
  $ echo b > b/b
  $ hg --cwd b ci -Amb -d'2 0'
  adding b

create a nested repository

  $ cd b
  $ hg init d
  $ echo d > d/d
  $ hg --cwd d ci -Amd -d'3 0'
  adding d
  $ cd ..
  $ hg init c
  $ echo c > c/c
  $ hg --cwd c ci -Amc -d'3 0'
  adding c

create repository without .hg/store

  $ hg init nostore
  $ rm -R nostore/.hg/store
  $ root=`pwd`
  $ cd ..
  $ cat > paths.conf <<EOF
  > [paths]
  > a=$root/a
  > b=$root/b
  > EOF
  $ hg serve -p $HGPORT -d --pid-file=hg.pid --webdir-conf paths.conf \
  >     -A access-paths.log -E error-paths-1.log
  $ cat hg.pid >> $DAEMON_PIDS

should give a 404 - file does not exist

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/a/file/tip/bork?style=raw'
  404 Not Found
  
  
  error: bork@8580ff50825a: not found in manifest
  [1]

should succeed

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/?style=raw'
  200 Script output follows
  
  
  /a/
  /b/
  
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/a/file/tip/a?style=raw'
  200 Script output follows
  
  a
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/b/file/tip/b?style=raw'
  200 Script output follows
  
  b

should give a 404 - repo is not published

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/c/file/tip/c?style=raw'
  404 Not Found
  
  
  error: repository c/file/tip/c not found
  [1]

atom-log without basedir

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/a/atom-log' | grep '<link'
   <link rel="self" href="http://*:$HGPORT/a/atom-log"/> (glob)
   <link rel="alternate" href="http://*:$HGPORT/a/"/> (glob)
    <link href="http://*:$HGPORT/a/rev/8580ff50825a"/> (glob)

rss-log without basedir

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/a/rss-log' | grep '<guid'
      <guid isPermaLink="true">http://*:$HGPORT/a/rev/8580ff50825a</guid> (glob)
  $ cat > paths.conf <<EOF
  > [paths]
  > t/a/=$root/a
  > b=$root/b
  > coll=$root/*
  > rcoll=$root/**
  > star=*
  > starstar=**
  > astar=webdir/a/*
  > EOF
  $ hg serve -p $HGPORT1 -d --pid-file=hg.pid --webdir-conf paths.conf \
  >     -A access-paths.log -E error-paths-2.log
  $ cat hg.pid >> $DAEMON_PIDS

should succeed, slashy names

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/?style=raw'
  200 Script output follows
  
  
  /t/a/
  /b/
  /coll/a/
  /coll/a/.hg/patches/
  /coll/b/
  /coll/c/
  /rcoll/a/
  /rcoll/a/.hg/patches/
  /rcoll/b/
  /rcoll/b/d/
  /rcoll/c/
  /star/webdir/a/
  /star/webdir/a/.hg/patches/
  /star/webdir/b/
  /star/webdir/c/
  /starstar/webdir/a/
  /starstar/webdir/a/.hg/patches/
  /starstar/webdir/b/
  /starstar/webdir/b/d/
  /starstar/webdir/c/
  /astar/
  /astar/.hg/patches/
  
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/?style=paper'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  
  <title>Mercurial repositories index</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" width=75 height=90 border=0 alt="mercurial" /></a>
  </div>
  <div class="main">
  <h2>Mercurial Repositories</h2>
  
  <table class="bigtable">
      <tr>
          <th><a href="?sort=name">Name</a></th>
          <th><a href="?sort=description">Description</a></th>
          <th><a href="?sort=contact">Contact</a></th>
          <th><a href="?sort=lastchange">Last modified</a></th>
          <th>&nbsp;</th>
      </tr>
      
  <tr class="parity0">
  <td><a href="/t/a/?style=paper">t/a</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity1">
  <td><a href="/b/?style=paper">b</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity0">
  <td><a href="/coll/a/?style=paper">coll/a</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity1">
  <td><a href="/coll/a/.hg/patches/?style=paper">coll/a/.hg/patches</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity0">
  <td><a href="/coll/b/?style=paper">coll/b</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity1">
  <td><a href="/coll/c/?style=paper">coll/c</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity0">
  <td><a href="/rcoll/a/?style=paper">rcoll/a</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity1">
  <td><a href="/rcoll/a/.hg/patches/?style=paper">rcoll/a/.hg/patches</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity0">
  <td><a href="/rcoll/b/?style=paper">rcoll/b</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity1">
  <td><a href="/rcoll/b/d/?style=paper">rcoll/b/d</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity0">
  <td><a href="/rcoll/c/?style=paper">rcoll/c</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity1">
  <td><a href="/star/webdir/a/?style=paper">star/webdir/a</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity0">
  <td><a href="/star/webdir/a/.hg/patches/?style=paper">star/webdir/a/.hg/patches</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity1">
  <td><a href="/star/webdir/b/?style=paper">star/webdir/b</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity0">
  <td><a href="/star/webdir/c/?style=paper">star/webdir/c</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity1">
  <td><a href="/starstar/webdir/a/?style=paper">starstar/webdir/a</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity0">
  <td><a href="/starstar/webdir/a/.hg/patches/?style=paper">starstar/webdir/a/.hg/patches</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity1">
  <td><a href="/starstar/webdir/b/?style=paper">starstar/webdir/b</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity0">
  <td><a href="/starstar/webdir/b/d/?style=paper">starstar/webdir/b/d</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity1">
  <td><a href="/starstar/webdir/c/?style=paper">starstar/webdir/c</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity0">
  <td><a href="/astar/?style=paper">astar</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  <tr class="parity1">
  <td><a href="/astar/.hg/patches/?style=paper">astar/.hg/patches</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  </table>
  </div>
  </div>
  
  
  </body>
  </html>
  
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/t?style=raw'
  200 Script output follows
  
  
  /t/a/
  
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/t/?style=raw'
  200 Script output follows
  
  
  /t/a/
  
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/t/?style=paper'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  
  <title>Mercurial repositories index</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" width=75 height=90 border=0 alt="mercurial" /></a>
  </div>
  <div class="main">
  <h2>Mercurial Repositories</h2>
  
  <table class="bigtable">
      <tr>
          <th><a href="?sort=name">Name</a></th>
          <th><a href="?sort=description">Description</a></th>
          <th><a href="?sort=contact">Contact</a></th>
          <th><a href="?sort=lastchange">Last modified</a></th>
          <th>&nbsp;</th>
      </tr>
      
  <tr class="parity0">
  <td><a href="/t/a/?style=paper">a</a></td>
  <td>unknown</td>
  <td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td>
  <td class="age">* ago</td> (glob)
  <td class="indexlinks"></td>
  </tr>
  
  </table>
  </div>
  </div>
  
  
  </body>
  </html>
  
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/t/a?style=atom'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <feed xmlns="http://www.w3.org/2005/Atom">
   <!-- Changelog -->
   <id>http://*:$HGPORT1/t/a/</id> (glob)
   <link rel="self" href="http://*:$HGPORT1/t/a/atom-log"/> (glob)
   <link rel="alternate" href="http://*:$HGPORT1/t/a/"/> (glob)
   <title>t/a Changelog</title>
   <updated>1970-01-01T00:00:01+00:00</updated>
  
   <entry>
    <title>a</title>
    <id>http://*:$HGPORT1/t/a/#changeset-8580ff50825a50c8f716709acdf8de0deddcd6ab</id> (glob)
    <link href="http://*:$HGPORT1/t/a/rev/8580ff50825a"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:01+00:00</updated>
    <published>1970-01-01T00:00:01+00:00</published>
    <content type="xhtml">
     <div xmlns="http://www.w3.org/1999/xhtml">
      <pre xml:space="preserve">a</pre>
     </div>
    </content>
   </entry>
  
  </feed>
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/t/a/?style=atom'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <feed xmlns="http://www.w3.org/2005/Atom">
   <!-- Changelog -->
   <id>http://*:$HGPORT1/t/a/</id> (glob)
   <link rel="self" href="http://*:$HGPORT1/t/a/atom-log"/> (glob)
   <link rel="alternate" href="http://*:$HGPORT1/t/a/"/> (glob)
   <title>t/a Changelog</title>
   <updated>1970-01-01T00:00:01+00:00</updated>
  
   <entry>
    <title>a</title>
    <id>http://*:$HGPORT1/t/a/#changeset-8580ff50825a50c8f716709acdf8de0deddcd6ab</id> (glob)
    <link href="http://*:$HGPORT1/t/a/rev/8580ff50825a"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:01+00:00</updated>
    <published>1970-01-01T00:00:01+00:00</published>
    <content type="xhtml">
     <div xmlns="http://www.w3.org/1999/xhtml">
      <pre xml:space="preserve">a</pre>
     </div>
    </content>
   </entry>
  
  </feed>
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/t/a/file/tip/a?style=raw'
  200 Script output follows
  
  a

Test [paths] '*' extension

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/coll/?style=raw'
  200 Script output follows
  
  
  /coll/a/
  /coll/a/.hg/patches/
  /coll/b/
  /coll/c/
  
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/coll/a/file/tip/a?style=raw'
  200 Script output follows
  
  a

Test [paths] '**' extension

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/rcoll/?style=raw'
  200 Script output follows
  
  
  /rcoll/a/
  /rcoll/a/.hg/patches/
  /rcoll/b/
  /rcoll/b/d/
  /rcoll/c/
  
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/rcoll/b/d/file/tip/d?style=raw'
  200 Script output follows
  
  d

Test [paths] '*' in a repo root

  $ hg id http://localhost:$HGPORT1/astar
  8580ff50825a

  $ "$TESTDIR/killdaemons.py"
  $ cat > paths.conf <<EOF
  > [paths]
  > t/a = $root/a
  > t/b = $root/b
  > c = $root/c
  > [web]
  > descend=false
  > EOF
  $ hg serve -p $HGPORT1 -d --pid-file=hg.pid --webdir-conf paths.conf \
  >     -A access-paths.log -E error-paths-3.log
  $ cat hg.pid >> $DAEMON_PIDS

test descend = False

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/?style=raw'
  200 Script output follows
  
  
  /c/
  
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/t/?style=raw'
  200 Script output follows
  
  
  /t/a/
  /t/b/
  
  $ "$TESTDIR/killdaemons.py"
  $ cat > paths.conf <<EOF
  > [paths]
  > nostore = $root/nostore
  > inexistent = $root/inexistent
  > EOF
  $ hg serve -p $HGPORT1 -d --pid-file=hg.pid --webdir-conf paths.conf \
  >     -A access-paths.log -E error-paths-4.log
  $ cat hg.pid >> $DAEMON_PIDS

test inexistent and inaccessible repo should be ignored silently

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT1 '/'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  
  <title>Mercurial repositories index</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" width=75 height=90 border=0 alt="mercurial" /></a>
  </div>
  <div class="main">
  <h2>Mercurial Repositories</h2>
  
  <table class="bigtable">
      <tr>
          <th><a href="?sort=name">Name</a></th>
          <th><a href="?sort=description">Description</a></th>
          <th><a href="?sort=contact">Contact</a></th>
          <th><a href="?sort=lastchange">Last modified</a></th>
          <th>&nbsp;</th>
      </tr>
      
  </table>
  </div>
  </div>
  
  
  </body>
  </html>
  
  $ cat > collections.conf <<EOF
  > [collections]
  > $root=$root
  > EOF
  $ hg serve --config web.baseurl=http://hg.example.com:8080/ -p $HGPORT2 -d \
  >     --pid-file=hg.pid --webdir-conf collections.conf \
  >     -A access-collections.log -E error-collections.log
  $ cat hg.pid >> $DAEMON_PIDS

collections: should succeed

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT2 '/?style=raw'
  200 Script output follows
  
  
  /a/
  /a/.hg/patches/
  /b/
  /c/
  
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT2 '/a/file/tip/a?style=raw'
  200 Script output follows
  
  a
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT2 '/b/file/tip/b?style=raw'
  200 Script output follows
  
  b
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT2 '/c/file/tip/c?style=raw'
  200 Script output follows
  
  c

atom-log with basedir /

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT2 '/a/atom-log' | grep '<link'
   <link rel="self" href="http://hg.example.com:8080/a/atom-log"/>
   <link rel="alternate" href="http://hg.example.com:8080/a/"/>
    <link href="http://hg.example.com:8080/a/rev/8580ff50825a"/>

rss-log with basedir /

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT2 '/a/rss-log' | grep '<guid'
      <guid isPermaLink="true">http://hg.example.com:8080/a/rev/8580ff50825a</guid>
  $ "$TESTDIR/killdaemons.py"
  $ hg serve --config web.baseurl=http://hg.example.com:8080/foo/ -p $HGPORT2 -d \
  >     --pid-file=hg.pid --webdir-conf collections.conf \
  >     -A access-collections-2.log -E error-collections-2.log
  $ cat hg.pid >> $DAEMON_PIDS

atom-log with basedir /foo/

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT2 '/a/atom-log' | grep '<link'
   <link rel="self" href="http://hg.example.com:8080/foo/a/atom-log"/>
   <link rel="alternate" href="http://hg.example.com:8080/foo/a/"/>
    <link href="http://hg.example.com:8080/foo/a/rev/8580ff50825a"/>

rss-log with basedir /foo/

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT2 '/a/rss-log' | grep '<guid'
      <guid isPermaLink="true">http://hg.example.com:8080/foo/a/rev/8580ff50825a</guid>

paths errors 1

  $ cat error-paths-1.log

paths errors 2

  $ cat error-paths-2.log

paths errors 3

  $ cat error-paths-3.log

collections errors

  $ cat error-collections.log

collections errors 2

  $ cat error-collections-2.log
