  $ "$TESTDIR/hghave" serve || exit 80

setting up repo

  $ hg init test
  $ cd test
  $ echo a > a
  $ echo b > b
  $ hg ci -Ama
  adding a
  adding b

change permissions for git diffs

  $ hg import -q --bypass - <<EOF
  > # HG changeset patch
  > # User test
  > # Date 0 0
  > b
  > 
  > diff --git a/a b/a
  > old mode 100644
  > new mode 100755
  > diff --git a/b b/b
  > deleted file mode 100644
  > --- a/b
  > +++ /dev/null
  > @@ -1,1 +0,0 @@
  > -b
  > EOF

set up hgweb

  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

revision

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'rev/0'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: 0cd96de13884</title>
  </head>
  <body>
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
   <li><a href="/shortlog/0cd96de13884">log</a></li>
   <li><a href="/graph/0cd96de13884">graph</a></li>
   <li><a href="/tags">tags</a></li>
   <li><a href="/bookmarks">bookmarks</a></li>
   <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
   <li class="active">changeset</li>
   <li><a href="/raw-rev/0cd96de13884">raw</a></li>
   <li><a href="/file/0cd96de13884">browse</a></li>
  </ul>
  <ul>
   
  </ul>
  <ul>
   <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  
  <h2><a href="/">test</a></h2>
  <h3>changeset 0:0cd96de13884   </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="description">a</div>
  
  <table id="changesetEntry">
  <tr>
   <th class="author">author</th>
   <td class="author">&#116;&#101;&#115;&#116;</td>
  </tr>
  <tr>
   <th class="date">date</th>
   <td class="date age">Thu, 01 Jan 1970 00:00:00 +0000</td></tr>
  <tr>
   <th class="author">parents</th>
   <td class="author"></td>
  </tr>
  <tr>
   <th class="author">children</th>
   <td class="author"> <a href="/rev/559edbd9ed20">559edbd9ed20</a></td>
  </tr>
  <tr>
   <th class="files">files</th>
   <td class="files"><a href="/file/0cd96de13884/a">a</a> <a href="/file/0cd96de13884/b">b</a> </td>
  </tr>
  <tr>
    <th class="diffstat">diffstat</th>
    <td class="diffstat">
       2 files changed, 2 insertions(+), 0 deletions(-)
  
      <a id="diffstatexpand" href="javascript:showDiffstat()"/>[<tt>+</tt>]</a>
      <div id="diffstatdetails" style="display:none;">
        <a href="javascript:hideDiffstat()"/>[<tt>-</tt>]</a>
        <p>
        <table>  <tr class="parity0">
      <td class="diffstat-file"><a href="#l1.1">a</a></td>
      <td class="diffstat-total" align="right">1</td>
      <td class="diffstat-graph">
        <span class="diffstat-add" style="width:100.0%;">&nbsp;</span>
        <span class="diffstat-remove" style="width:0.0%;">&nbsp;</span>
      </td>
    </tr>
    <tr class="parity1">
      <td class="diffstat-file"><a href="#l2.1">b</a></td>
      <td class="diffstat-total" align="right">1</td>
      <td class="diffstat-graph">
        <span class="diffstat-add" style="width:100.0%;">&nbsp;</span>
        <span class="diffstat-remove" style="width:0.0%;">&nbsp;</span>
      </td>
    </tr>
  </table>
      </div>
    </td>
  </tr>
  <tr>
   <th class="author">change baseline</th>
   <td class="author"></td>
  </tr>
  <tr>
   <th class="author">current baseline</th>
   <td class="author"><a href="/rev/000000000000">000000000000</a></td>
  </tr>
  </table>
  
  <div class="overflow">
  <div class="sourcefirst">   line diff</div>
  
  <div class="source bottomline parity0"><pre><a href="#l1.1" id="l1.1">     1.1</a> <span class="minusline">--- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  </span><a href="#l1.2" id="l1.2">     1.2</a> <span class="plusline">+++ b/a	Thu Jan 01 00:00:00 1970 +0000
  </span><a href="#l1.3" id="l1.3">     1.3</a> <span class="atline">@@ -0,0 +1,1 @@
  </span><a href="#l1.4" id="l1.4">     1.4</a> <span class="plusline">+a
  </span></pre></div><div class="source bottomline parity1"><pre><a href="#l2.1" id="l2.1">     2.1</a> <span class="minusline">--- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  </span><a href="#l2.2" id="l2.2">     2.2</a> <span class="plusline">+++ b/b	Thu Jan 01 00:00:00 1970 +0000
  </span><a href="#l2.3" id="l2.3">     2.3</a> <span class="atline">@@ -0,0 +1,1 @@
  </span><a href="#l2.4" id="l2.4">     2.4</a> <span class="plusline">+b
  </span></pre></div>
  </div>
  
  </div>
  </div>
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

raw revision

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'raw-rev/0'
  200 Script output follows
  
  
  # HG changeset patch
  # User test
  # Date 0 0
  # Node ID 0cd96de13884b090099512d4794ae87ad067ea8e
  
  a
  
  diff -r 000000000000 -r 0cd96de13884 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r 000000000000 -r 0cd96de13884 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  

diff removed file

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'diff/tip/b'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: b diff</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/559edbd9ed20">log</a></li>
  <li><a href="/graph/559edbd9ed20">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/559edbd9ed20">changeset</a></li>
  <li><a href="/file/559edbd9ed20">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/559edbd9ed20/b">file</a></li>
  <li><a href="/file/tip/b">latest</a></li>
  <li class="active">diff</li>
  <li><a href="/comparison/559edbd9ed20/b">comparison</a></li>
  <li><a href="/annotate/559edbd9ed20/b">annotate</a></li>
  <li><a href="/log/559edbd9ed20/b">file log</a></li>
  <li><a href="/raw-file/559edbd9ed20/b">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>diff b @ 1:559edbd9ed20</h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="description">b</div>
  
  <table id="changesetEntry">
  <tr>
   <th>author</th>
   <td>&#116;&#101;&#115;&#116;</td>
  </tr>
  <tr>
   <th>date</th>
   <td class="date age">Thu, 01 Jan 1970 00:00:00 +0000</td>
  </tr>
  <tr>
   <th>parents</th>
   <td><a href="/file/0cd96de13884/b">0cd96de13884</a> </td>
  </tr>
  <tr>
   <th>children</th>
   <td></td>
  </tr>
  
  </table>
  
  <div class="overflow">
  <div class="sourcefirst">   line diff</div>
  
  <div class="source bottomline parity0"><pre><a href="#l1.1" id="l1.1">     1.1</a> <span class="minusline">--- a/b	Thu Jan 01 00:00:00 1970 +0000
  </span><a href="#l1.2" id="l1.2">     1.2</a> <span class="plusline">+++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  </span><a href="#l1.3" id="l1.3">     1.3</a> <span class="atline">@@ -1,1 +0,0 @@
  </span><a href="#l1.4" id="l1.4">     1.4</a> <span class="minusline">-b
  </span></pre></div>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

set up hgweb with git diffs

  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS
  $ hg serve --config 'diff.git=1' -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

revision

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'rev/0'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: 0cd96de13884</title>
  </head>
  <body>
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
   <li><a href="/shortlog/0cd96de13884">log</a></li>
   <li><a href="/graph/0cd96de13884">graph</a></li>
   <li><a href="/tags">tags</a></li>
   <li><a href="/bookmarks">bookmarks</a></li>
   <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
   <li class="active">changeset</li>
   <li><a href="/raw-rev/0cd96de13884">raw</a></li>
   <li><a href="/file/0cd96de13884">browse</a></li>
  </ul>
  <ul>
   
  </ul>
  <ul>
   <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  
  <h2><a href="/">test</a></h2>
  <h3>changeset 0:0cd96de13884   </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="description">a</div>
  
  <table id="changesetEntry">
  <tr>
   <th class="author">author</th>
   <td class="author">&#116;&#101;&#115;&#116;</td>
  </tr>
  <tr>
   <th class="date">date</th>
   <td class="date age">Thu, 01 Jan 1970 00:00:00 +0000</td></tr>
  <tr>
   <th class="author">parents</th>
   <td class="author"></td>
  </tr>
  <tr>
   <th class="author">children</th>
   <td class="author"> <a href="/rev/559edbd9ed20">559edbd9ed20</a></td>
  </tr>
  <tr>
   <th class="files">files</th>
   <td class="files"><a href="/file/0cd96de13884/a">a</a> <a href="/file/0cd96de13884/b">b</a> </td>
  </tr>
  <tr>
    <th class="diffstat">diffstat</th>
    <td class="diffstat">
       2 files changed, 2 insertions(+), 0 deletions(-)
  
      <a id="diffstatexpand" href="javascript:showDiffstat()"/>[<tt>+</tt>]</a>
      <div id="diffstatdetails" style="display:none;">
        <a href="javascript:hideDiffstat()"/>[<tt>-</tt>]</a>
        <p>
        <table>  <tr class="parity0">
      <td class="diffstat-file"><a href="#l1.1">a</a></td>
      <td class="diffstat-total" align="right">1</td>
      <td class="diffstat-graph">
        <span class="diffstat-add" style="width:100.0%;">&nbsp;</span>
        <span class="diffstat-remove" style="width:0.0%;">&nbsp;</span>
      </td>
    </tr>
    <tr class="parity1">
      <td class="diffstat-file"><a href="#l2.1">b</a></td>
      <td class="diffstat-total" align="right">1</td>
      <td class="diffstat-graph">
        <span class="diffstat-add" style="width:100.0%;">&nbsp;</span>
        <span class="diffstat-remove" style="width:0.0%;">&nbsp;</span>
      </td>
    </tr>
  </table>
      </div>
    </td>
  </tr>
  <tr>
   <th class="author">change baseline</th>
   <td class="author"></td>
  </tr>
  <tr>
   <th class="author">current baseline</th>
   <td class="author"><a href="/rev/000000000000">000000000000</a></td>
  </tr>
  </table>
  
  <div class="overflow">
  <div class="sourcefirst">   line diff</div>
  
  <div class="source bottomline parity0"><pre><a href="#l1.1" id="l1.1">     1.1</a> new file mode 100644
  <a href="#l1.2" id="l1.2">     1.2</a> <span class="minusline">--- /dev/null
  </span><a href="#l1.3" id="l1.3">     1.3</a> <span class="plusline">+++ b/a
  </span><a href="#l1.4" id="l1.4">     1.4</a> <span class="atline">@@ -0,0 +1,1 @@
  </span><a href="#l1.5" id="l1.5">     1.5</a> <span class="plusline">+a
  </span></pre></div><div class="source bottomline parity1"><pre><a href="#l2.1" id="l2.1">     2.1</a> new file mode 100644
  <a href="#l2.2" id="l2.2">     2.2</a> <span class="minusline">--- /dev/null
  </span><a href="#l2.3" id="l2.3">     2.3</a> <span class="plusline">+++ b/b
  </span><a href="#l2.4" id="l2.4">     2.4</a> <span class="atline">@@ -0,0 +1,1 @@
  </span><a href="#l2.5" id="l2.5">     2.5</a> <span class="plusline">+b
  </span></pre></div>
  </div>
  
  </div>
  </div>
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

revision

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'raw-rev/0'
  200 Script output follows
  
  
  # HG changeset patch
  # User test
  # Date 0 0
  # Node ID 0cd96de13884b090099512d4794ae87ad067ea8e
  
  a
  
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,1 @@
  +a
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +b
  

diff removed file

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'diff/tip/a'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: a diff</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/559edbd9ed20">log</a></li>
  <li><a href="/graph/559edbd9ed20">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/559edbd9ed20">changeset</a></li>
  <li><a href="/file/559edbd9ed20">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/559edbd9ed20/a">file</a></li>
  <li><a href="/file/tip/a">latest</a></li>
  <li class="active">diff</li>
  <li><a href="/comparison/559edbd9ed20/a">comparison</a></li>
  <li><a href="/annotate/559edbd9ed20/a">annotate</a></li>
  <li><a href="/log/559edbd9ed20/a">file log</a></li>
  <li><a href="/raw-file/559edbd9ed20/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>diff a @ 1:559edbd9ed20</h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="description">b</div>
  
  <table id="changesetEntry">
  <tr>
   <th>author</th>
   <td>&#116;&#101;&#115;&#116;</td>
  </tr>
  <tr>
   <th>date</th>
   <td class="date age">Thu, 01 Jan 1970 00:00:00 +0000</td>
  </tr>
  <tr>
   <th>parents</th>
   <td></td>
  </tr>
  <tr>
   <th>children</th>
   <td></td>
  </tr>
  
  </table>
  
  <div class="overflow">
  <div class="sourcefirst">   line diff</div>
  
  <div class="source bottomline parity0"><pre><a href="#l1.1" id="l1.1">     1.1</a> old mode 100644
  <a href="#l1.2" id="l1.2">     1.2</a> new mode 100755
  </pre></div>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

comparison new file

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'comparison/0/a'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: a comparison</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/0cd96de13884">log</a></li>
  <li><a href="/graph/0cd96de13884">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/0cd96de13884">changeset</a></li>
  <li><a href="/file/0cd96de13884">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/0cd96de13884/a">file</a></li>
  <li><a href="/file/tip/a">latest</a></li>
  <li><a href="/diff/0cd96de13884/a">diff</a></li>
  <li class="active">comparison</li>
  <li><a href="/annotate/0cd96de13884/a">annotate</a></li>
  <li><a href="/log/0cd96de13884/a">file log</a></li>
  <li><a href="/raw-file/0cd96de13884/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>comparison a @ 0:0cd96de13884</h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="description">a</div>
  
  <table id="changesetEntry">
  <tr>
   <th>author</th>
   <td>&#116;&#101;&#115;&#116;</td>
  </tr>
  <tr>
   <th>date</th>
   <td class="date age">Thu, 01 Jan 1970 00:00:00 +0000</td>
  </tr>
  <tr>
   <th>parents</th>
   <td></td>
  </tr>
  <tr>
   <th>children</th>
   <td></td>
  </tr>
  
  </table>
  
  <div class="overflow">
  <div class="sourcefirst">   comparison</div>
  <div class="legend">
    <span class="legendinfo equal">equal</span>
    <span class="legendinfo delete">deleted</span>
    <span class="legendinfo insert">inserted</span>
    <span class="legendinfo replace">replaced</span>
  </div>
  
  <table class="bigtable">
    <thead class="header">
      <tr>
        <th>-1:000000000000</th>
        <th>0:b789fdd96dc2</th>
      </tr>
    </thead>
    
  <tbody class="block">
  
  <tr>
  <td class="source insert"><a href="#r1" id="r1">      </a> </td>
  <td class="source insert"><a href="#r1" id="r1">     1</a> a</td>
  </tr>
  </tbody>
  </table>
  
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

comparison existing file

  $ hg up
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a >> a
  $ hg ci -mc
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'comparison/tip/a'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: a comparison</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/d73db4d812ff">log</a></li>
  <li><a href="/graph/d73db4d812ff">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/d73db4d812ff">changeset</a></li>
  <li><a href="/file/d73db4d812ff">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/d73db4d812ff/a">file</a></li>
  <li><a href="/file/tip/a">latest</a></li>
  <li><a href="/diff/d73db4d812ff/a">diff</a></li>
  <li class="active">comparison</li>
  <li><a href="/annotate/d73db4d812ff/a">annotate</a></li>
  <li><a href="/log/d73db4d812ff/a">file log</a></li>
  <li><a href="/raw-file/d73db4d812ff/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>comparison a @ 2:d73db4d812ff</h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="description">c</div>
  
  <table id="changesetEntry">
  <tr>
   <th>author</th>
   <td>&#116;&#101;&#115;&#116;</td>
  </tr>
  <tr>
   <th>date</th>
   <td class="date age">Thu, 01 Jan 1970 00:00:00 +0000</td>
  </tr>
  <tr>
   <th>parents</th>
   <td><a href="/file/0cd96de13884/a">0cd96de13884</a> </td>
  </tr>
  <tr>
   <th>children</th>
   <td></td>
  </tr>
  
  </table>
  
  <div class="overflow">
  <div class="sourcefirst">   comparison</div>
  <div class="legend">
    <span class="legendinfo equal">equal</span>
    <span class="legendinfo delete">deleted</span>
    <span class="legendinfo insert">inserted</span>
    <span class="legendinfo replace">replaced</span>
  </div>
  
  <table class="bigtable">
    <thead class="header">
      <tr>
        <th>0:b789fdd96dc2</th>
        <th>1:a80d06849b33</th>
      </tr>
    </thead>
    
  <tbody class="block">
  
  <tr>
  <td class="source equal"><a href="#l1r1" id="l1r1">     1</a> a</td>
  <td class="source equal"><a href="#l1r1" id="l1r1">     1</a> a</td>
  </tr>
  <tr>
  <td class="source insert"><a href="#r2" id="r2">      </a> </td>
  <td class="source insert"><a href="#r2" id="r2">     2</a> a</td>
  </tr>
  </tbody>
  </table>
  
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

comparison removed file

  $ hg rm a
  $ hg ci -md
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'comparison/tip/a'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: a comparison</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/20e80271eb7a">log</a></li>
  <li><a href="/graph/20e80271eb7a">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/20e80271eb7a">changeset</a></li>
  <li><a href="/file/20e80271eb7a">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/20e80271eb7a/a">file</a></li>
  <li><a href="/file/tip/a">latest</a></li>
  <li><a href="/diff/20e80271eb7a/a">diff</a></li>
  <li class="active">comparison</li>
  <li><a href="/annotate/20e80271eb7a/a">annotate</a></li>
  <li><a href="/log/20e80271eb7a/a">file log</a></li>
  <li><a href="/raw-file/20e80271eb7a/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>comparison a @ 3:20e80271eb7a</h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="description">d</div>
  
  <table id="changesetEntry">
  <tr>
   <th>author</th>
   <td>&#116;&#101;&#115;&#116;</td>
  </tr>
  <tr>
   <th>date</th>
   <td class="date age">Thu, 01 Jan 1970 00:00:00 +0000</td>
  </tr>
  <tr>
   <th>parents</th>
   <td><a href="/file/0cd96de13884/a">0cd96de13884</a> </td>
  </tr>
  <tr>
   <th>children</th>
   <td></td>
  </tr>
  
  </table>
  
  <div class="overflow">
  <div class="sourcefirst">   comparison</div>
  <div class="legend">
    <span class="legendinfo equal">equal</span>
    <span class="legendinfo delete">deleted</span>
    <span class="legendinfo insert">inserted</span>
    <span class="legendinfo replace">replaced</span>
  </div>
  
  <table class="bigtable">
    <thead class="header">
      <tr>
        <th>1:a80d06849b33</th>
        <th>-1:000000000000</th>
      </tr>
    </thead>
    
  <tbody class="block">
  
  <tr>
  <td class="source delete"><a href="#l1" id="l1">     1</a> a</td>
  <td class="source delete"><a href="#l1" id="l1">      </a> </td>
  </tr>
  <tr>
  <td class="source delete"><a href="#l2" id="l2">     2</a> a</td>
  <td class="source delete"><a href="#l2" id="l2">      </a> </td>
  </tr>
  </tbody>
  </table>
  
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

  $ cd ..

test import rev as raw-rev

  $ hg clone -r0 test test1
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd test1
  $ hg import -q --bypass --exact http://localhost:$HGPORT/rev/1

raw revision with diff block numbers

  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS
  $ cat <<EOF > .hg/hgrc
  > [web]
  > templates = rawdiff
  > EOF
  $ mkdir rawdiff
  $ cat <<EOF > rawdiff/map
  > mimetype = 'text/plain; charset={encoding}'
  > changeset = '{diff}'
  > difflineplus = '{line}'
  > difflineminus = '{line}'
  > difflineat = '{line}'
  > diffline = '{line}'
  > filenodelink = ''
  > filenolink = ''
  > fileline = '{line}'
  > diffblock = 'Block: {blockno}\n{lines}\n'
  > EOF
  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'raw-rev/0'
  200 Script output follows
  
  Block: 1
  diff -r 000000000000 -r 0cd96de13884 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  Block: 2
  diff -r 000000000000 -r 0cd96de13884 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS
  $ rm .hg/hgrc rawdiff/map
  $ rmdir rawdiff
  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

errors

  $ cat ../test/errors.log

  $ cd ..
