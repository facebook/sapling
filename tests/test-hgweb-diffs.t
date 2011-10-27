setting up repo

  $ hg init test
  $ cd test
  $ echo a > a
  $ echo b > b
  $ hg ci -Ama
  adding a
  adding b

change permissions for git diffs

  $ chmod 755 a
  $ hg ci -Amb

set up hgweb

  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

revision

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/rev/0'
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
   <td class="author"> <a href="/rev/78e4ebad7cdf">78e4ebad7cdf</a></td>
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

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/raw-rev/0'
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

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/diff/tip/a'
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
  <li><a href="/shortlog/78e4ebad7cdf">log</a></li>
  <li><a href="/graph/78e4ebad7cdf">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/78e4ebad7cdf">changeset</a></li>
  <li><a href="/file/78e4ebad7cdf">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/78e4ebad7cdf/a">file</a></li>
  <li><a href="/file/tip/a">latest</a></li>
  <li class="active">diff</li>
  <li><a href="/annotate/78e4ebad7cdf/a">annotate</a></li>
  <li><a href="/log/78e4ebad7cdf/a">file log</a></li>
  <li><a href="/raw-file/78e4ebad7cdf/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>diff a @ 1:78e4ebad7cdf</h3>
  
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
  
  <div class="source bottomline parity0"><pre><a href="#l1.1" id="l1.1">     1.1</a> <span class="minusline">--- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  </span><a href="#l1.2" id="l1.2">     1.2</a> <span class="plusline">+++ b/a	Thu Jan 01 00:00:00 1970 +0000
  </span><a href="#l1.3" id="l1.3">     1.3</a> <span class="atline">@@ -0,0 +1,1 @@
  </span><a href="#l1.4" id="l1.4">     1.4</a> <span class="plusline">+a
  </span></pre></div>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

set up hgweb with git diffs

  $ "$TESTDIR/killdaemons.py"
  $ hg serve --config 'diff.git=1' -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

revision

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/rev/0'
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
   <td class="author"> <a href="/rev/78e4ebad7cdf">78e4ebad7cdf</a></td>
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

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/raw-rev/0'
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

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/diff/tip/a'
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
  <li><a href="/shortlog/78e4ebad7cdf">log</a></li>
  <li><a href="/graph/78e4ebad7cdf">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/78e4ebad7cdf">changeset</a></li>
  <li><a href="/file/78e4ebad7cdf">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/78e4ebad7cdf/a">file</a></li>
  <li><a href="/file/tip/a">latest</a></li>
  <li class="active">diff</li>
  <li><a href="/annotate/78e4ebad7cdf/a">annotate</a></li>
  <li><a href="/log/78e4ebad7cdf/a">file log</a></li>
  <li><a href="/raw-file/78e4ebad7cdf/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>diff a @ 1:78e4ebad7cdf</h3>
  
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
  
  <div class="source bottomline parity0"><pre><a href="#l1.1" id="l1.1">     1.1</a> new file mode 100755
  <a href="#l1.2" id="l1.2">     1.2</a> <span class="minusline">--- /dev/null
  </span><a href="#l1.3" id="l1.3">     1.3</a> <span class="plusline">+++ b/a
  </span><a href="#l1.4" id="l1.4">     1.4</a> <span class="atline">@@ -0,0 +1,1 @@
  </span><a href="#l1.5" id="l1.5">     1.5</a> <span class="plusline">+a
  </span></pre></div>
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
  $ hg import -q --exact http://localhost:$HGPORT/rev/1

errors

  $ cat ../test/errors.log
