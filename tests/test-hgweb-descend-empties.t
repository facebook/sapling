#require serve

Test chains of near empty directories, terminating 3 different ways:
- a1: file at level 4 (deepest)
- b1: two dirs at level 3
- d1: file at level 2

Set up the repo

  $ hg init test
  $ cd test
  $ mkdir -p a1/a2/a3/a4
  $ mkdir -p b1/b2/b3/b4
  $ mkdir -p b1/b2/b3/c4
  $ mkdir -p d1/d2/d3/d4
  $ echo foo > a1/a2/a3/a4/foo
  $ echo foo > b1/b2/b3/b4/foo
  $ echo foo > b1/b2/b3/c4/foo
  $ echo foo > d1/d2/d3/d4/foo
  $ echo foo > d1/d2/foo
  $ hg ci -Ama
  adding a1/a2/a3/a4/foo
  adding b1/b2/b3/b4/foo
  adding b1/b2/b3/c4/foo
  adding d1/d2/d3/d4/foo
  adding d1/d2/foo
  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

manifest with descending (paper)

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: c9f45f7a1659 /</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/c9f45f7a1659">log</a></li>
  <li><a href="/graph/c9f45f7a1659">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/c9f45f7a1659">changeset</a></li>
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
  <h3>directory / @ 0:c9f45f7a1659 <span class="tag">tip</span> </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <table class="bigtable">
  <thead>
  <tr>
    <th class="name">name</th>
    <th class="size">size</th>
    <th class="permissions">permissions</th>
  </tr>
  </thead>
  <tbody class="stripes2">
  <tr class="fileline">
    <td class="name"><a href="/file/c9f45f7a1659/">[up]</a></td>
    <td class="size"></td>
    <td class="permissions">drwxr-xr-x</td>
  </tr>
  
  <tr class="fileline">
  <td class="name">
  <a href="/file/c9f45f7a1659/a1">
  <img src="/static/coal-folder.png" alt="dir."/> a1/
  </a>
  <a href="/file/c9f45f7a1659/a1/a2/a3/a4">
  a2/a3/a4
  </a>
  </td>
  <td class="size"></td>
  <td class="permissions">drwxr-xr-x</td>
  </tr>
  <tr class="fileline">
  <td class="name">
  <a href="/file/c9f45f7a1659/b1">
  <img src="/static/coal-folder.png" alt="dir."/> b1/
  </a>
  <a href="/file/c9f45f7a1659/b1/b2/b3">
  b2/b3
  </a>
  </td>
  <td class="size"></td>
  <td class="permissions">drwxr-xr-x</td>
  </tr>
  <tr class="fileline">
  <td class="name">
  <a href="/file/c9f45f7a1659/d1">
  <img src="/static/coal-folder.png" alt="dir."/> d1/
  </a>
  <a href="/file/c9f45f7a1659/d1/d2">
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
  

manifest with descending (coal)

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file?style=coal'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-coal.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: c9f45f7a1659 /</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/c9f45f7a1659?style=coal">log</a></li>
  <li><a href="/graph/c9f45f7a1659?style=coal">graph</a></li>
  <li><a href="/tags?style=coal">tags</a></li>
  <li><a href="/bookmarks?style=coal">bookmarks</a></li>
  <li><a href="/branches?style=coal">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/c9f45f7a1659?style=coal">changeset</a></li>
  <li class="active">browse</li>
  </ul>
  <ul>
  
  </ul>
  <ul>
   <li><a href="/help?style=coal">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>directory / @ 0:c9f45f7a1659 <span class="tag">tip</span> </h3>
  
  <form class="search" action="/log">
  <input type="hidden" name="style" value="coal" />
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <table class="bigtable">
  <thead>
  <tr>
    <th class="name">name</th>
    <th class="size">size</th>
    <th class="permissions">permissions</th>
  </tr>
  </thead>
  <tbody class="stripes2">
  <tr class="fileline">
    <td class="name"><a href="/file/c9f45f7a1659/?style=coal">[up]</a></td>
    <td class="size"></td>
    <td class="permissions">drwxr-xr-x</td>
  </tr>
  
  <tr class="fileline parity1">
  <td class="name">
  <a href="/file/c9f45f7a1659/a1?style=coal">
  <img src="/static/coal-folder.png" alt="dir."/> a1/
  </a>
  <a href="/file/c9f45f7a1659/a1/a2/a3/a4?style=coal">
  a2/a3/a4
  </a>
  </td>
  <td class="size"></td>
  <td class="permissions">drwxr-xr-x</td>
  </tr>
  <tr class="fileline parity0">
  <td class="name">
  <a href="/file/c9f45f7a1659/b1?style=coal">
  <img src="/static/coal-folder.png" alt="dir."/> b1/
  </a>
  <a href="/file/c9f45f7a1659/b1/b2/b3?style=coal">
  b2/b3
  </a>
  </td>
  <td class="size"></td>
  <td class="permissions">drwxr-xr-x</td>
  </tr>
  <tr class="fileline parity1">
  <td class="name">
  <a href="/file/c9f45f7a1659/d1?style=coal">
  <img src="/static/coal-folder.png" alt="dir."/> d1/
  </a>
  <a href="/file/c9f45f7a1659/d1/d2?style=coal">
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
  

manifest with descending (monoblue)

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file?style=monoblue'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en" lang="en">
  <head>
      <link rel="icon" href="/static/hgicon.png" type="image/png" />
      <meta name="robots" content="index, nofollow"/>
      <link rel="stylesheet" href="/static/style-monoblue.css" type="text/css" />
      <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: files</title>
      <link rel="alternate" type="application/atom+xml" href="/atom-log" title="Atom feed for test"/>
      <link rel="alternate" type="application/rss+xml" href="/rss-log" title="RSS feed for test"/>
  </head>
  
  <body>
  <div id="container">
      <div class="page-header">
          <h1 class="breadcrumb"><a href="/">Mercurial</a>  / files</h1>
  
          <form action="/log">
              <input type="hidden" name="style" value="monoblue" />
              <dl class="search">
                  <dt><label>Search: </label></dt>
                  <dd><input type="text" name="rev" /></dd>
              </dl>
          </form>
  
          <ul class="page-nav">
              <li><a href="/summary?style=monoblue">summary</a></li>
              <li><a href="/shortlog?style=monoblue">shortlog</a></li>
              <li><a href="/changelog?style=monoblue">changelog</a></li>
              <li><a href="/graph/c9f45f7a1659?style=monoblue">graph</a></li>
              <li><a href="/tags?style=monoblue">tags</a></li>
              <li><a href="/bookmarks?style=monoblue">bookmarks</a></li>
              <li><a href="/branches?style=monoblue">branches</a></li>
              <li class="current">files</li>
              <li><a href="/help?style=monoblue">help</a></li>
          </ul>
      </div>
  
      <ul class="submenu">
          <li><a href="/rev/c9f45f7a1659?style=monoblue">changeset</a></li>
          
      </ul>
  
      <h2 class="no-link no-border">files</h2>
      <p class="files">/ <span class="logtags"><span class="branchtag" title="default">default</span> <span class="tagtag" title="tip">tip</span> </span></p>
  
      <table>
          <tr class="parity0">
              <td>drwxr-xr-x</td>
              <td></td>
              <td></td>
              <td><a href="/file/c9f45f7a1659/?style=monoblue">[up]</a></td>
              <td class="link">&nbsp;</td>
          </tr>
          
  <tr class="parity1">
  <td>drwxr-xr-x</td>
  <td></td>
  <td></td>
  <td>
  <a href="/file/c9f45f7a1659/a1?style=monoblue">a1</a>
  <a href="/file/c9f45f7a1659/a1/a2/a3/a4?style=monoblue">a2/a3/a4</a>
  </td>
  <td><a href="/file/c9f45f7a1659/a1?style=monoblue">files</a></td>
  </tr>
  <tr class="parity0">
  <td>drwxr-xr-x</td>
  <td></td>
  <td></td>
  <td>
  <a href="/file/c9f45f7a1659/b1?style=monoblue">b1</a>
  <a href="/file/c9f45f7a1659/b1/b2/b3?style=monoblue">b2/b3</a>
  </td>
  <td><a href="/file/c9f45f7a1659/b1?style=monoblue">files</a></td>
  </tr>
  <tr class="parity1">
  <td>drwxr-xr-x</td>
  <td></td>
  <td></td>
  <td>
  <a href="/file/c9f45f7a1659/d1?style=monoblue">d1</a>
  <a href="/file/c9f45f7a1659/d1/d2?style=monoblue">d2</a>
  </td>
  <td><a href="/file/c9f45f7a1659/d1?style=monoblue">files</a></td>
  </tr>
          
      </table>
  
      <script type="text/javascript">process_dates()</script>
      <div class="page-footer">
          <p>Mercurial Repository: test</p>
          <ul class="rss-logo">
              <li><a href="/rss-log">RSS</a></li>
              <li><a href="/atom-log">Atom</a></li>
          </ul>
          
      </div>
  
      <div id="powered-by">
          <p><a href="http://mercurial.selenic.com/" title="Mercurial"><img src="/static/hglogo.png" width=75 height=90 border=0 alt="mercurial" /></a></p>
      </div>
  
      <div id="corner-top-left"></div>
      <div id="corner-top-right"></div>
      <div id="corner-bottom-left"></div>
      <div id="corner-bottom-right"></div>
  
  </div>
  
  </body>
  </html>
  

manifest with descending (gitweb)

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file?style=gitweb'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US" lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow"/>
  <link rel="stylesheet" href="/static/style-gitweb.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: files</title>
  <link rel="alternate" type="application/atom+xml"
     href="/atom-log" title="Atom feed for test"/>
  <link rel="alternate" type="application/rss+xml"
     href="/rss-log" title="RSS feed for test"/>
  </head>
  <body>
  
  <div class="page_header">
  <a href="http://mercurial.selenic.com/" title="Mercurial" style="float: right;">Mercurial</a>
  <a href="/">Mercurial</a>  / files
  </div>
  
  <div class="page_nav">
  <a href="/summary?style=gitweb">summary</a> |
  <a href="/shortlog?style=gitweb">shortlog</a> |
  <a href="/log?style=gitweb">changelog</a> |
  <a href="/graph?style=gitweb">graph</a> |
  <a href="/tags?style=gitweb">tags</a> |
  <a href="/bookmarks?style=gitweb">bookmarks</a> |
  <a href="/branches?style=gitweb">branches</a> |
  files |
  <a href="/rev/c9f45f7a1659?style=gitweb">changeset</a>  |
  <a href="/help?style=gitweb">help</a>
  <br/>
  </div>
  
  <div class="title">/ <span class="logtags"><span class="branchtag" title="default">default</span> <span class="tagtag" title="tip">tip</span> </span></div>
  <table cellspacing="0">
  <tr class="parity0">
  <td style="font-family:monospace">drwxr-xr-x</td>
  <td style="font-family:monospace"></td>
  <td style="font-family:monospace"></td>
  <td><a href="/file/c9f45f7a1659/?style=gitweb">[up]</a></td>
  <td class="link">&nbsp;</td>
  </tr>
  
  <tr class="parity1">
  <td style="font-family:monospace">drwxr-xr-x</td>
  <td style="font-family:monospace"></td>
  <td style="font-family:monospace"></td>
  <td>
  <a href="/file/c9f45f7a1659/a1?style=gitweb">a1</a>
  <a href="/file/c9f45f7a1659/a1/a2/a3/a4?style=gitweb">a2/a3/a4</a>
  </td>
  <td class="link">
  <a href="/file/c9f45f7a1659/a1?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity0">
  <td style="font-family:monospace">drwxr-xr-x</td>
  <td style="font-family:monospace"></td>
  <td style="font-family:monospace"></td>
  <td>
  <a href="/file/c9f45f7a1659/b1?style=gitweb">b1</a>
  <a href="/file/c9f45f7a1659/b1/b2/b3?style=gitweb">b2/b3</a>
  </td>
  <td class="link">
  <a href="/file/c9f45f7a1659/b1?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity1">
  <td style="font-family:monospace">drwxr-xr-x</td>
  <td style="font-family:monospace"></td>
  <td style="font-family:monospace"></td>
  <td>
  <a href="/file/c9f45f7a1659/d1?style=gitweb">d1</a>
  <a href="/file/c9f45f7a1659/d1/d2?style=gitweb">d2</a>
  </td>
  <td class="link">
  <a href="/file/c9f45f7a1659/d1?style=gitweb">files</a>
  </td>
  </tr>
  
  </table>
  
  <script type="text/javascript">process_dates()</script>
  <div class="page_footer">
  <div class="page_footer_text">test</div>
  <div class="rss_logo">
  <a href="/rss-log">RSS</a>
  <a href="/atom-log">Atom</a>
  </div>
  <br />
  
  </div>
  </body>
  </html>
  

manifest with descending (spartan)

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file?style=spartan'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd">
  <html>
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png">
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: files for changeset c9f45f7a1659</title>
  </head>
  <body>
  
  <div class="buttons">
  <a href="/log/0?style=spartan">changelog</a>
  <a href="/shortlog/0?style=spartan">shortlog</a>
  <a href="/graph?style=spartan">graph</a>
  <a href="/tags?style=spartan">tags</a>
  <a href="/branches?style=spartan">branches</a>
  <a href="/rev/c9f45f7a1659?style=spartan">changeset</a>
  
  <a href="/help?style=spartan">help</a>
  </div>
  
  <h2><a href="/">Mercurial</a>  / files for changeset <a href="/rev/c9f45f7a1659">c9f45f7a1659</a>: /</h2>
  
  <table cellpadding="0" cellspacing="0">
  <tr class="parity0">
    <td><tt>drwxr-xr-x</tt>&nbsp;
    <td>&nbsp;
    <td>&nbsp;
    <td><a href="/file/c9f45f7a1659/?style=spartan">[up]</a>
  </tr>
  
  <tr class="parity1">
  <td><tt>drwxr-xr-x</tt>&nbsp;
  <td>&nbsp;
  <td>&nbsp;
  <td>
  <a href="/file/c9f45f7a1659/a1?style=spartan">a1/</a>
  <a href="/file/c9f45f7a1659/a1/a2/a3/a4?style=spartan">
  a2/a3/a4
  </a>
  <tr class="parity0">
  <td><tt>drwxr-xr-x</tt>&nbsp;
  <td>&nbsp;
  <td>&nbsp;
  <td>
  <a href="/file/c9f45f7a1659/b1?style=spartan">b1/</a>
  <a href="/file/c9f45f7a1659/b1/b2/b3?style=spartan">
  b2/b3
  </a>
  <tr class="parity1">
  <td><tt>drwxr-xr-x</tt>&nbsp;
  <td>&nbsp;
  <td>&nbsp;
  <td>
  <a href="/file/c9f45f7a1659/d1?style=spartan">d1/</a>
  <a href="/file/c9f45f7a1659/d1/d2?style=spartan">
  d2
  </a>
  
  </table>
  <script type="text/javascript">process_dates()</script>
  
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" width=75 height=90 border=0 alt="mercurial"></a>
  </div>
  
  </body>
  </html>
  

  $ cat errors.log

  $ cd ..
