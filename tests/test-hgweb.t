Some tests for hgweb. Tests static files, plain files and different 404's.

  $ hg init test
  $ cd test
  $ mkdir da
  $ echo foo > da/foo
  $ echo foo > foo
  $ hg ci -Ambase
  adding da/foo
  adding foo
  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

manifest

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/file/tip/?style=raw')
  200 Script output follows
  
  
  drwxr-xr-x da
  -rw-r--r-- 4 foo
  
  
  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/file/tip/da?style=raw')
  200 Script output follows
  
  
  -rw-r--r-- 4 foo
  
  

plain file

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/file/tip/foo?style=raw'
  200 Script output follows
  
  foo

should give a 404 - static file that does not exist

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/static/bogus'
  404 Not Found
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: error</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" width=75 height=90 border=0 alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog">log</a></li>
  <li><a href="/graph">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  
  <h2><a href="/">test</a></h2>
  <h3>error</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30"></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="description">
  <p>
  An error occurred while processing your request:
  </p>
  <p>
  Not Found
  </p>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  [1]

should give a 404 - bad revision

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/file/spam/foo?style=raw'
  404 Not Found
  
  
  error: revision not found: spam
  [1]

should give a 400 - bad command

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/file/tip/foo?cmd=spam&style=raw'
  400* (glob)
  
  
  error: no such method: spam
  [1]

should give a 404 - file does not exist

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/file/tip/bork?style=raw'
  404 Not Found
  
  
  error: bork@2ef0ac749a14: not found in manifest
  [1]
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/file/tip/bork'
  404 Not Found
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: error</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" width=75 height=90 border=0 alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog">log</a></li>
  <li><a href="/graph">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  
  <h2><a href="/">test</a></h2>
  <h3>error</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30"></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="description">
  <p>
  An error occurred while processing your request:
  </p>
  <p>
  bork@2ef0ac749a14: not found in manifest
  </p>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  [1]
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/diff/tip/bork?style=raw'
  404 Not Found
  
  
  error: bork@2ef0ac749a14: not found in manifest
  [1]

try bad style

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/file/tip/?style=foobar')
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: 2ef0ac749a14 /</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/2ef0ac749a14">log</a></li>
  <li><a href="/graph/2ef0ac749a14">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/2ef0ac749a14">changeset</a></li>
  <li class="active">browse</li>
  </ul>
  <ul>
  
  </ul>
  <ul>
   <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>directory / @ 0:2ef0ac749a14 <span class="tag">tip</span> </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <table class="bigtable">
  <tr>
    <th class="name">name</th>
    <th class="size">size</th>
    <th class="permissions">permissions</th>
  </tr>
  <tr class="fileline parity0">
    <td class="name"><a href="/file/2ef0ac749a14/">[up]</a></td>
    <td class="size"></td>
    <td class="permissions">drwxr-xr-x</td>
  </tr>
  
  <tr class="fileline parity1">
  <td class="name">
  <a href="/file/2ef0ac749a14/da">
  <img src="/static/coal-folder.png" alt="dir."/> da/
  </a>
  <a href="/file/2ef0ac749a14/da/">
  
  </a>
  </td>
  <td class="size"></td>
  <td class="permissions">drwxr-xr-x</td>
  </tr>
  
  <tr class="fileline parity0">
  <td class="filename">
  <a href="/file/2ef0ac749a14/foo">
  <img src="/static/coal-file.png" alt="file"/> foo
  </a>
  </td>
  <td class="size">4</td>
  <td class="permissions">-rw-r--r--</td>
  </tr>
  </table>
  </div>
  </div>
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

stop and restart

  $ "$TESTDIR/killdaemons.py"
  $ hg serve -p $HGPORT -d --pid-file=hg.pid -A access.log
  $ cat hg.pid >> $DAEMON_PIDS

Test the access/error files are opened in append mode

  $ python -c "print len(file('access.log').readlines()), 'log lines written'"
  10 log lines written

static file

  $ "$TESTDIR/get-with-headers.py" --twice localhost:$HGPORT '/static/style-gitweb.css'
  200 Script output follows
  
  body { font-family: sans-serif; font-size: 12px; margin:0px; border:solid #d9d8d1; border-width:1px; margin:10px; }
  a { color:#0000cc; }
  a:hover, a:visited, a:active { color:#880000; }
  div.page_header { height:25px; padding:8px; font-size:18px; font-weight:bold; background-color:#d9d8d1; }
  div.page_header a:visited { color:#0000cc; }
  div.page_header a:hover { color:#880000; }
  div.page_nav { padding:8px; }
  div.page_nav a:visited { color:#0000cc; }
  div.page_path { padding:8px; border:solid #d9d8d1; border-width:0px 0px 1px}
  div.page_footer { padding:4px 8px; background-color: #d9d8d1; }
  div.page_footer_text { float:left; color:#555555; font-style:italic; }
  div.page_body { padding:8px; }
  div.title, a.title {
  	display:block; padding:6px 8px;
  	font-weight:bold; background-color:#edece6; text-decoration:none; color:#000000;
  }
  a.title:hover { background-color: #d9d8d1; }
  div.title_text { padding:6px 0px; border: solid #d9d8d1; border-width:0px 0px 1px; }
  div.log_body { padding:8px 8px 8px 150px; }
  .age { white-space:nowrap; }
  span.age { position:relative; float:left; width:142px; font-style:italic; }
  div.log_link {
  	padding:0px 8px;
  	font-size:10px; font-family:sans-serif; font-style:normal;
  	position:relative; float:left; width:136px;
  }
  div.list_head { padding:6px 8px 4px; border:solid #d9d8d1; border-width:1px 0px 0px; font-style:italic; }
  a.list { text-decoration:none; color:#000000; }
  a.list:hover { text-decoration:underline; color:#880000; }
  table { padding:8px 4px; }
  th { padding:2px 5px; font-size:12px; text-align:left; }
  tr.light:hover, .parity0:hover { background-color:#edece6; }
  tr.dark, .parity1 { background-color:#f6f6f0; }
  tr.dark:hover, .parity1:hover { background-color:#edece6; }
  td { padding:2px 5px; font-size:12px; vertical-align:top; }
  td.closed { background-color: #99f; }
  td.link { padding:2px 5px; font-family:sans-serif; font-size:10px; }
  td.indexlinks { white-space: nowrap; }
  td.indexlinks a {
    padding: 2px 5px; line-height: 10px;
    border: 1px solid;
    color: #ffffff; background-color: #7777bb;
    border-color: #aaaadd #333366 #333366 #aaaadd;
    font-weight: bold;  text-align: center; text-decoration: none;
    font-size: 10px;
  }
  td.indexlinks a:hover { background-color: #6666aa; }
  div.pre { font-family:monospace; font-size:12px; white-space:pre; }
  div.diff_info { font-family:monospace; color:#000099; background-color:#edece6; font-style:italic; }
  div.index_include { border:solid #d9d8d1; border-width:0px 0px 1px; padding:12px 8px; }
  div.search { margin:4px 8px; position:absolute; top:56px; right:12px }
  .linenr { color:#999999; text-decoration:none }
  div.rss_logo { float: right; white-space: nowrap; }
  div.rss_logo a {
  	padding:3px 6px; line-height:10px;
  	border:1px solid; border-color:#fcc7a5 #7d3302 #3e1a01 #ff954e;
  	color:#ffffff; background-color:#ff6600;
  	font-weight:bold; font-family:sans-serif; font-size:10px;
  	text-align:center; text-decoration:none;
  }
  div.rss_logo a:hover { background-color:#ee5500; }
  pre { margin: 0; }
  span.logtags span {
  	padding: 0px 4px;
  	font-size: 10px;
  	font-weight: normal;
  	border: 1px solid;
  	background-color: #ffaaff;
  	border-color: #ffccff #ff00ee #ff00ee #ffccff;
  }
  span.logtags span.tagtag {
  	background-color: #ffffaa;
  	border-color: #ffffcc #ffee00 #ffee00 #ffffcc;
  }
  span.logtags span.branchtag {
  	background-color: #aaffaa;
  	border-color: #ccffcc #00cc33 #00cc33 #ccffcc;
  }
  span.logtags span.inbranchtag {
  	background-color: #d5dde6;
  	border-color: #e3ecf4 #9398f4 #9398f4 #e3ecf4;
  }
  span.logtags span.bookmarktag {
  	background-color: #afdffa;
  	border-color: #ccecff #46ace6 #46ace6 #ccecff;
  }
  
  /* Graph */
  div#wrapper {
  	position: relative;
  	margin: 0;
  	padding: 0;
  	margin-top: 3px;
  }
  
  canvas {
  	position: absolute;
  	z-index: 5;
  	top: -0.9em;
  	margin: 0;
  }
  
  ul#nodebgs {
  	list-style: none inside none;
  	padding: 0;
  	margin: 0;
  	top: -0.7em;
  }
  
  ul#graphnodes li, ul#nodebgs li {
  	height: 39px;
  }
  
  ul#graphnodes {
  	position: absolute;
  	z-index: 10;
  	top: -0.8em;
  	list-style: none inside none;
  	padding: 0;
  }
  
  ul#graphnodes li .info {
  	display: block;
  	font-size: 100%;
  	position: relative;
  	top: -3px;
  	font-style: italic;
  }
  304 Not Modified
  

errors

  $ cat errors.log
