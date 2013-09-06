  $ "$TESTDIR/hghave" serve || exit 80

Test chains of near empty directories, terminating 3 different ways:
- a1: file at level 4 (deepest)
- b1: two dirs at level 3
- e1: file at level 2

Set up the repo

  $ hg init test
  $ cd test
  $ mkdir -p a1/a2/a3/a4
  $ mkdir -p b1/b2/b3/b4
  $ mkdir -p b1/b2/c3/c4
  $ mkdir -p d1/d2/d3/d4
  $ echo foo > a1/a2/a3/a4/foo
  $ echo foo > b1/b2/b3/b4/foo
  $ echo foo > b1/b2/c3/c4/foo
  $ echo foo > d1/d2/d3/d4/foo
  $ echo foo > d1/d2/foo
  $ hg ci -Ama
  adding a1/a2/a3/a4/foo
  adding b1/b2/b3/b4/foo
  adding b1/b2/c3/c4/foo
  adding d1/d2/d3/d4/foo
  adding d1/d2/foo
  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

manifest with descending

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: 9087c84a0f5d /</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/9087c84a0f5d">log</a></li>
  <li><a href="/graph/9087c84a0f5d">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/9087c84a0f5d">changeset</a></li>
  <li class="active">browse</li>
  </ul>
  <ul>
  
  </ul>
  <ul>
   <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>directory / @ 0:9087c84a0f5d <span class="tag">tip</span> </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <table class="bigtable">
  <tr>
    <th class="name">name</th>
    <th class="size">size</th>
    <th class="permissions">permissions</th>
  </tr>
  <tbody class="stripes2">
  <tr class="fileline">
    <td class="name"><a href="/file/9087c84a0f5d/">[up]</a></td>
    <td class="size"></td>
    <td class="permissions">drwxr-xr-x</td>
  </tr>
  
  <tr class="fileline">
  <td class="name">
  <a href="/file/9087c84a0f5d/a1">
  <img src="/static/coal-folder.png" alt="dir."/> a1/
  </a>
  <a href="/file/9087c84a0f5d/a1/a2/a3/a4">
  a2/a3/a4
  </a>
  </td>
  <td class="size"></td>
  <td class="permissions">drwxr-xr-x</td>
  </tr>
  <tr class="fileline">
  <td class="name">
  <a href="/file/9087c84a0f5d/b1">
  <img src="/static/coal-folder.png" alt="dir."/> b1/
  </a>
  <a href="/file/9087c84a0f5d/b1/b2">
  b2
  </a>
  </td>
  <td class="size"></td>
  <td class="permissions">drwxr-xr-x</td>
  </tr>
  <tr class="fileline">
  <td class="name">
  <a href="/file/9087c84a0f5d/d1">
  <img src="/static/coal-folder.png" alt="dir."/> d1/
  </a>
  <a href="/file/9087c84a0f5d/d1/d2">
  d2
  </a>
  </td>
  <td class="size"></td>
  <td class="permissions">drwxr-xr-x</td>
  </tr>
  
  </tbody>
  </table>
  </div>
  </div>
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

  $ cat errors.log

  $ cd ..
