
  $ hg init test
  $ cd test
  $ echo b > b
  $ hg ci -Am "b"
  adding b
  $ echo a > a
  $ hg ci -Am "first a"
  adding a
  $ hg rm a
  $ hg ci -m "del a"
  $ echo b > a
  $ hg ci -Am "second a"
  adding a
  $ hg rm a
  $ hg ci -m "del2 a"
  $ hg mv b c
  $ hg ci -m "mv b"
  $ echo c >> c
  $ hg ci -m "change c"
  $ hg log -p
  changeset:   6:b7682196df1c
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change c
  
  diff -r 1a6696706df2 -r b7682196df1c c
  --- a/c	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   b
  +c
  
  changeset:   5:1a6696706df2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     mv b
  
  diff -r 52e848cdcd88 -r 1a6696706df2 b
  --- a/b	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -b
  diff -r 52e848cdcd88 -r 1a6696706df2 c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  changeset:   4:52e848cdcd88
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     del2 a
  
  diff -r 01de2d66a28d -r 52e848cdcd88 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -b
  
  changeset:   3:01de2d66a28d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     second a
  
  diff -r be3ebcc91739 -r 01de2d66a28d a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  changeset:   2:be3ebcc91739
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     del a
  
  diff -r 5ed941583260 -r be3ebcc91739 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  
  changeset:   1:5ed941583260
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     first a
  
  diff -r 6563da9dcf87 -r 5ed941583260 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  changeset:   0:6563da9dcf87
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  diff -r 000000000000 -r 6563da9dcf87 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

tip - two revisions

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/log/tip/a')
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  
  <title>test: a history</title>
  <link rel="alternate" type="application/atom+xml"
     href="/atom-log/tip/a" title="Atom feed for test:a" />
  <link rel="alternate" type="application/rss+xml"
     href="/rss-log/tip/a" title="RSS feed for test:a" />
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/01de2d66a28d">log</a></li>
  <li><a href="/graph/01de2d66a28d">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/01de2d66a28d">changeset</a></li>
  <li><a href="/file/01de2d66a28d">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/01de2d66a28d/a">file</a></li>
  <li><a href="/diff/01de2d66a28d/a">diff</a></li>
  <li><a href="/annotate/01de2d66a28d/a">annotate</a></li>
  <li class="active">file log</li>
  <li><a href="/raw-file/01de2d66a28d/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>log a</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="navigate">
  <a href="/log/01de2d66a28d/a?revcount=30">less</a>
  <a href="/log/01de2d66a28d/a?revcount=120">more</a>
  | <a href="/log/5ed941583260/a">(0)</a> <a href="/log/tip/a">tip</a> </div>
  
  <table class="bigtable">
   <tr>
    <th class="age">age</th>
    <th class="author">author</th>
    <th class="description">description</th>
   </tr>
   <tr class="parity0">
    <td class="age">1970-01-01</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/01de2d66a28d">second a</a></td>
   </tr>
   <tr class="parity1">
    <td class="age">1970-01-01</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/5ed941583260">first a</a></td>
   </tr>
  
  </table>
  
  <div class="navigate">
  <a href="/log/01de2d66a28d/a?revcount=30">less</a>
  <a href="/log/01de2d66a28d/a?revcount=120">more</a>
  | <a href="/log/5ed941583260/a">(0)</a> <a href="/log/tip/a">tip</a> 
  </div>
  
  </div>
  </div>
  
  
  
  </body>
  </html>
  

second version - two revisions

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/log/3/a')
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  
  <title>test: a history</title>
  <link rel="alternate" type="application/atom+xml"
     href="/atom-log/tip/a" title="Atom feed for test:a" />
  <link rel="alternate" type="application/rss+xml"
     href="/rss-log/tip/a" title="RSS feed for test:a" />
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/01de2d66a28d">log</a></li>
  <li><a href="/graph/01de2d66a28d">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/01de2d66a28d">changeset</a></li>
  <li><a href="/file/01de2d66a28d">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/01de2d66a28d/a">file</a></li>
  <li><a href="/diff/01de2d66a28d/a">diff</a></li>
  <li><a href="/annotate/01de2d66a28d/a">annotate</a></li>
  <li class="active">file log</li>
  <li><a href="/raw-file/01de2d66a28d/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>log a</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="navigate">
  <a href="/log/01de2d66a28d/a?revcount=30">less</a>
  <a href="/log/01de2d66a28d/a?revcount=120">more</a>
  | <a href="/log/5ed941583260/a">(0)</a> <a href="/log/tip/a">tip</a> </div>
  
  <table class="bigtable">
   <tr>
    <th class="age">age</th>
    <th class="author">author</th>
    <th class="description">description</th>
   </tr>
   <tr class="parity0">
    <td class="age">1970-01-01</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/01de2d66a28d">second a</a></td>
   </tr>
   <tr class="parity1">
    <td class="age">1970-01-01</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/5ed941583260">first a</a></td>
   </tr>
  
  </table>
  
  <div class="navigate">
  <a href="/log/01de2d66a28d/a?revcount=30">less</a>
  <a href="/log/01de2d66a28d/a?revcount=120">more</a>
  | <a href="/log/5ed941583260/a">(0)</a> <a href="/log/tip/a">tip</a> 
  </div>
  
  </div>
  </div>
  
  
  
  </body>
  </html>
  

first deleted - one revision

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/log/2/a')
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  
  <title>test: a history</title>
  <link rel="alternate" type="application/atom+xml"
     href="/atom-log/tip/a" title="Atom feed for test:a" />
  <link rel="alternate" type="application/rss+xml"
     href="/rss-log/tip/a" title="RSS feed for test:a" />
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/5ed941583260">log</a></li>
  <li><a href="/graph/5ed941583260">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/5ed941583260">changeset</a></li>
  <li><a href="/file/5ed941583260">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/5ed941583260/a">file</a></li>
  <li><a href="/diff/5ed941583260/a">diff</a></li>
  <li><a href="/annotate/5ed941583260/a">annotate</a></li>
  <li class="active">file log</li>
  <li><a href="/raw-file/5ed941583260/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>log a</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="navigate">
  <a href="/log/5ed941583260/a?revcount=30">less</a>
  <a href="/log/5ed941583260/a?revcount=120">more</a>
  | <a href="/log/5ed941583260/a">(0)</a> <a href="/log/tip/a">tip</a> </div>
  
  <table class="bigtable">
   <tr>
    <th class="age">age</th>
    <th class="author">author</th>
    <th class="description">description</th>
   </tr>
   <tr class="parity0">
    <td class="age">1970-01-01</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/5ed941583260">first a</a></td>
   </tr>
  
  </table>
  
  <div class="navigate">
  <a href="/log/5ed941583260/a?revcount=30">less</a>
  <a href="/log/5ed941583260/a?revcount=120">more</a>
  | <a href="/log/5ed941583260/a">(0)</a> <a href="/log/tip/a">tip</a> 
  </div>
  
  </div>
  </div>
  
  
  
  </body>
  </html>
  

first version - one revision

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/log/1/a')
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  
  <title>test: a history</title>
  <link rel="alternate" type="application/atom+xml"
     href="/atom-log/tip/a" title="Atom feed for test:a" />
  <link rel="alternate" type="application/rss+xml"
     href="/rss-log/tip/a" title="RSS feed for test:a" />
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/5ed941583260">log</a></li>
  <li><a href="/graph/5ed941583260">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/5ed941583260">changeset</a></li>
  <li><a href="/file/5ed941583260">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/5ed941583260/a">file</a></li>
  <li><a href="/diff/5ed941583260/a">diff</a></li>
  <li><a href="/annotate/5ed941583260/a">annotate</a></li>
  <li class="active">file log</li>
  <li><a href="/raw-file/5ed941583260/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>log a</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="navigate">
  <a href="/log/5ed941583260/a?revcount=30">less</a>
  <a href="/log/5ed941583260/a?revcount=120">more</a>
  | <a href="/log/5ed941583260/a">(0)</a> <a href="/log/tip/a">tip</a> </div>
  
  <table class="bigtable">
   <tr>
    <th class="age">age</th>
    <th class="author">author</th>
    <th class="description">description</th>
   </tr>
   <tr class="parity0">
    <td class="age">1970-01-01</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/5ed941583260">first a</a></td>
   </tr>
  
  </table>
  
  <div class="navigate">
  <a href="/log/5ed941583260/a?revcount=30">less</a>
  <a href="/log/5ed941583260/a?revcount=120">more</a>
  | <a href="/log/5ed941583260/a">(0)</a> <a href="/log/tip/a">tip</a> 
  </div>
  
  </div>
  </div>
  
  
  
  </body>
  </html>
  

before addition - error

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/log/0/a')
  404 Not Found
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  
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
  a@6563da9dcf87: not found in manifest
  </p>
  </div>
  </div>
  </div>
  
  
  
  </body>
  </html>
  
  [1]

should show base link, use spartan because it shows it

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/log/tip/c?style=spartan')
  200 Script output follows
  
  <!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN">
  <html>
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png">
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style.css" type="text/css" />
  
  <title>test: c history</title>
  <link rel="alternate" type="application/atom+xml"
     href="/atom-log/tip/c" title="Atom feed for test:c">
  <link rel="alternate" type="application/rss+xml"
     href="/rss-log/tip/c" title="RSS feed for test:c">
  </head>
  <body>
  
  <div class="buttons">
  <a href="/log?style=spartan">changelog</a>
  <a href="/shortlog?style=spartan">shortlog</a>
  <a href="/graph?style=spartan">graph</a>
  <a href="/tags?style=spartan">tags</a>
  <a href="/branches?style=spartan">branches</a>
  <a href="/file/b7682196df1c/c?style=spartan">file</a>
  <a href="/annotate/b7682196df1c/c?style=spartan">annotate</a>
  <a href="/help?style=spartan">help</a>
  <a type="application/rss+xml" href="/rss-log/tip/c">rss</a>
  <a type="application/atom+xml" href="/atom-log/tip/c" title="Atom feed for test:c">atom</a>
  </div>
  
  <h2>c revision history</h2>
  
  <p>navigate: <small class="navigate"><a href="/log/1a6696706df2/c?style=spartan">(0)</a> <a href="/log/tip/c?style=spartan">tip</a> </small></p>
  
  <table class="logEntry parity0">
   <tr>
    <th class="age">1970-01-01:</th>
    <th class="firstline"><a href="/rev/b7682196df1c?style=spartan">change c</a></th>
   </tr>
   <tr>
    <th class="revision">revision 1:</td>
    <td class="node">
     <a href="/file/b7682196df1c/c?style=spartan">b7682196df1c</a>
     <a href="/diff/b7682196df1c/c?style=spartan">(diff)</a>
     <a href="/annotate/b7682196df1c/c?style=spartan">(annotate)</a>
    </td>
   </tr>
   
   <tr>
    <th class="author">author:</th>
    <td class="author">&#116;&#101;&#115;&#116;</td>
   </tr>
   <tr>
    <th class="date">date:</th>
    <td class="date">Thu Jan 01 00:00:00 1970 +0000</td>
   </tr>
  </table>
  
  
  <table class="logEntry parity1">
   <tr>
    <th class="age">1970-01-01:</th>
    <th class="firstline"><a href="/rev/1a6696706df2?style=spartan">mv b</a></th>
   </tr>
   <tr>
    <th class="revision">revision 0:</td>
    <td class="node">
     <a href="/file/1a6696706df2/c?style=spartan">1a6696706df2</a>
     <a href="/diff/1a6696706df2/c?style=spartan">(diff)</a>
     <a href="/annotate/1a6696706df2/c?style=spartan">(annotate)</a>
    </td>
   </tr>
   
  <tr>
  <th>base:</th>
  <td>
  <a href="/file/1e88685f5dde/b?style=spartan">
  b@1e88685f5dde
  </a>
  </td>
  </tr>
   <tr>
    <th class="author">author:</th>
    <td class="author">&#116;&#101;&#115;&#116;</td>
   </tr>
   <tr>
    <th class="date">date:</th>
    <td class="date">Thu Jan 01 00:00:00 1970 +0000</td>
   </tr>
  </table>
  
  
  
  
  
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" width=75 height=90 border=0 alt="mercurial"></a>
  </div>
  
  </body>
  </html>
  

rss log

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/rss-log/tip/a')
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <rss version="2.0">
    <channel>
      <link>http://*:$HGPORT/</link> (glob)
      <language>en-us</language>
  
      <title>test: a history</title>
      <description>a revision history</description>
      <item>
      <title>second a</title>
      <link>http://*:$HGPORT/log01de2d66a28d/a</link> (glob)
      <description><![CDATA[second a]]></description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  <item>
      <title>first a</title>
      <link>http://*:$HGPORT/log5ed941583260/a</link> (glob)
      <description><![CDATA[first a]]></description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  
    </channel>
  </rss>

atom log

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/atom-log/tip/a')
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <feed xmlns="http://www.w3.org/2005/Atom">
   <id>http://*:$HGPORT/atom-log/tip/a</id> (glob)
   <link rel="self" href="http://*:$HGPORT/atom-log/tip/a"/> (glob)
   <title>test: a history</title>
   <updated>1970-01-01T00:00:00+00:00</updated>
  
   <entry>
    <title>second a</title>
    <id>http://*:$HGPORT/#changeset-01de2d66a28df5549090991dccda788726948517</id> (glob)
    <link href="http://*:$HGPORT/rev/01de2d66a28d"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
     <div xmlns="http://www.w3.org/1999/xhtml">
      <pre xml:space="preserve">second a</pre>
     </div>
    </content>
   </entry>
   <entry>
    <title>first a</title>
    <id>http://*:$HGPORT/#changeset-5ed941583260248620985524192fdc382ef57c36</id> (glob)
    <link href="http://*:$HGPORT/rev/5ed941583260"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
     <div xmlns="http://www.w3.org/1999/xhtml">
      <pre xml:space="preserve">first a</pre>
     </div>
    </content>
   </entry>
  
  </feed>

errors

  $ cat errors.log
