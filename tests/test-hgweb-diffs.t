#require serve

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

  $ get-with-headers.py localhost:$HGPORT 'rev/0'
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
  <a href="https://mercurial-scm.org/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
   <li><a href="/shortlog/0">log</a></li>
   <li><a href="/graph/0">graph</a></li>
   <li><a href="/tags">tags</a></li>
   <li><a href="/bookmarks">bookmarks</a></li>
   <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
   <li class="active">changeset</li>
   <li><a href="/raw-rev/0">raw</a></li>
   <li><a href="/file/0">browse</a></li>
  </ul>
  <ul>
   
  </ul>
  <ul>
   <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>
   changeset 0:<a href="/rev/0cd96de13884">0cd96de13884</a>
   
  </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <div class="description">a</div>
  
  <table id="changesetEntry">
  <tr>
   <th class="author">author</th>
   <td class="author">&#116;&#101;&#115;&#116;</td>
  </tr>
  <tr>
   <th class="date">date</th>
   <td class="date age">Thu, 01 Jan 1970 00:00:00 +0000</td>
  </tr>
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
  
      <a id="diffstatexpand" href="javascript:toggleDiffstat()">[<tt>+</tt>]</a>
      <div id="diffstatdetails" style="display:none;">
        <a href="javascript:toggleDiffstat()">[<tt>-</tt>]</a>
        <table class="diffstat-table stripes2">  <tr>
      <td class="diffstat-file"><a href="#l1.1">a</a></td>
      <td class="diffstat-total" align="right">1</td>
      <td class="diffstat-graph">
        <span class="diffstat-add" style="width:100.0%;">&nbsp;</span>
        <span class="diffstat-remove" style="width:0.0%;">&nbsp;</span>
      </td>
    </tr>
    <tr>
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
  <div class="sourcefirst linewraptoggle">line wrap: <a class="linewraplink" href="javascript:toggleLinewrap()">on</a></div>
  <div class="sourcefirst"> line diff</div>
  <div class="stripes2 diffblocks">
  <div class="bottomline inc-lineno"><pre class="sourcelines wrap">
  <span id="l1.1" class="minusline">--- /dev/null	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l1.1"></a>
  <span id="l1.2" class="plusline">+++ b/a	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l1.2"></a>
  <span id="l1.3" class="atline">@@ -0,0 +1,1 @@</span><a href="#l1.3"></a>
  <span id="l1.4" class="plusline">+a</span><a href="#l1.4"></a></pre></div><div class="bottomline inc-lineno"><pre class="sourcelines wrap">
  <span id="l2.1" class="minusline">--- /dev/null	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l2.1"></a>
  <span id="l2.2" class="plusline">+++ b/b	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l2.2"></a>
  <span id="l2.3" class="atline">@@ -0,0 +1,1 @@</span><a href="#l2.3"></a>
  <span id="l2.4" class="plusline">+b</span><a href="#l2.4"></a></pre></div>
  </div>
  </div>
  
  </div>
  </div>
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

raw revision

  $ get-with-headers.py localhost:$HGPORT 'raw-rev/0'
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

  $ hg log --template "{file_mods}\n{file_dels}\n" -r tip
  a
  b
  $ hg parents --template "{node|short}\n" -r tip
  0cd96de13884
  $ hg parents --template "{node|short}\n" -r tip b
  0cd96de13884

  $ get-with-headers.py localhost:$HGPORT 'diff/tip/b'
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
  <a href="https://mercurial-scm.org/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/tip">log</a></li>
  <li><a href="/graph/tip">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/tip">changeset</a></li>
  <li><a href="/file/tip">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/tip/b">file</a></li>
  <li><a href="/file/tip/b">latest</a></li>
  <li class="active">diff</li>
  <li><a href="/comparison/tip/b">comparison</a></li>
  <li><a href="/annotate/tip/b">annotate</a></li>
  <li><a href="/log/tip/b">file log</a></li>
  <li><a href="/raw-file/tip/b">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>
   diff b @ 1:<a href="/rev/559edbd9ed20">559edbd9ed20</a>
   <span class="tag">tip</span> 
  </h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
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
  <div class="sourcefirst linewraptoggle">line wrap: <a class="linewraplink" href="javascript:toggleLinewrap()">on</a></div>
  <div class="sourcefirst"> line diff</div>
  <div class="stripes2 diffblocks">
  <div class="bottomline inc-lineno"><pre class="sourcelines wrap">
  <span id="l1.1" class="minusline">--- a/b	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l1.1"></a>
  <span id="l1.2" class="plusline">+++ /dev/null	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l1.2"></a>
  <span id="l1.3" class="atline">@@ -1,1 +0,0 @@</span><a href="#l1.3"></a>
  <span id="l1.4" class="minusline">-b</span><a href="#l1.4"></a></pre></div>
  </div>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

set up hgweb with git diffs

  $ killdaemons.py
  $ hg serve --config 'diff.git=1' -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

revision

  $ get-with-headers.py localhost:$HGPORT 'rev/0'
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
  <a href="https://mercurial-scm.org/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
   <li><a href="/shortlog/0">log</a></li>
   <li><a href="/graph/0">graph</a></li>
   <li><a href="/tags">tags</a></li>
   <li><a href="/bookmarks">bookmarks</a></li>
   <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
   <li class="active">changeset</li>
   <li><a href="/raw-rev/0">raw</a></li>
   <li><a href="/file/0">browse</a></li>
  </ul>
  <ul>
   
  </ul>
  <ul>
   <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>
   changeset 0:<a href="/rev/0cd96de13884">0cd96de13884</a>
   
  </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <div class="description">a</div>
  
  <table id="changesetEntry">
  <tr>
   <th class="author">author</th>
   <td class="author">&#116;&#101;&#115;&#116;</td>
  </tr>
  <tr>
   <th class="date">date</th>
   <td class="date age">Thu, 01 Jan 1970 00:00:00 +0000</td>
  </tr>
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
  
      <a id="diffstatexpand" href="javascript:toggleDiffstat()">[<tt>+</tt>]</a>
      <div id="diffstatdetails" style="display:none;">
        <a href="javascript:toggleDiffstat()">[<tt>-</tt>]</a>
        <table class="diffstat-table stripes2">  <tr>
      <td class="diffstat-file"><a href="#l1.1">a</a></td>
      <td class="diffstat-total" align="right">1</td>
      <td class="diffstat-graph">
        <span class="diffstat-add" style="width:100.0%;">&nbsp;</span>
        <span class="diffstat-remove" style="width:0.0%;">&nbsp;</span>
      </td>
    </tr>
    <tr>
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
  <div class="sourcefirst linewraptoggle">line wrap: <a class="linewraplink" href="javascript:toggleLinewrap()">on</a></div>
  <div class="sourcefirst"> line diff</div>
  <div class="stripes2 diffblocks">
  <div class="bottomline inc-lineno"><pre class="sourcelines wrap">
  <span id="l1.1">new file mode 100644</span><a href="#l1.1"></a>
  <span id="l1.2" class="minusline">--- /dev/null</span><a href="#l1.2"></a>
  <span id="l1.3" class="plusline">+++ b/a</span><a href="#l1.3"></a>
  <span id="l1.4" class="atline">@@ -0,0 +1,1 @@</span><a href="#l1.4"></a>
  <span id="l1.5" class="plusline">+a</span><a href="#l1.5"></a></pre></div><div class="bottomline inc-lineno"><pre class="sourcelines wrap">
  <span id="l2.1">new file mode 100644</span><a href="#l2.1"></a>
  <span id="l2.2" class="minusline">--- /dev/null</span><a href="#l2.2"></a>
  <span id="l2.3" class="plusline">+++ b/b</span><a href="#l2.3"></a>
  <span id="l2.4" class="atline">@@ -0,0 +1,1 @@</span><a href="#l2.4"></a>
  <span id="l2.5" class="plusline">+b</span><a href="#l2.5"></a></pre></div>
  </div>
  </div>
  
  </div>
  </div>
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

revision

  $ get-with-headers.py localhost:$HGPORT 'raw-rev/0'
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
  

diff modified file

  $ hg log --template "{file_mods}\n{file_dels}\n" -r tip
  a
  b
  $ hg parents --template "{node|short}\n" -r tip
  0cd96de13884
  $ hg parents --template "{node|short}\n" -r tip a
  0cd96de13884

  $ get-with-headers.py localhost:$HGPORT 'diff/tip/a'
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
  <a href="https://mercurial-scm.org/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/tip">log</a></li>
  <li><a href="/graph/tip">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/tip">changeset</a></li>
  <li><a href="/file/tip">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/tip/a">file</a></li>
  <li><a href="/file/tip/a">latest</a></li>
  <li class="active">diff</li>
  <li><a href="/comparison/tip/a">comparison</a></li>
  <li><a href="/annotate/tip/a">annotate</a></li>
  <li><a href="/log/tip/a">file log</a></li>
  <li><a href="/raw-file/tip/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>
   diff a @ 1:<a href="/rev/559edbd9ed20">559edbd9ed20</a>
   <span class="tag">tip</span> 
  </h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
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
   <td><a href="/file/0cd96de13884/a">0cd96de13884</a> </td>
  </tr>
  <tr>
   <th>children</th>
   <td></td>
  </tr>
  </table>
  
  <div class="overflow">
  <div class="sourcefirst linewraptoggle">line wrap: <a class="linewraplink" href="javascript:toggleLinewrap()">on</a></div>
  <div class="sourcefirst"> line diff</div>
  <div class="stripes2 diffblocks">
  <div class="bottomline inc-lineno"><pre class="sourcelines wrap">
  <span id="l1.1">old mode 100644</span><a href="#l1.1"></a>
  <span id="l1.2">new mode 100755</span><a href="#l1.2"></a></pre></div>
  </div>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

comparison new file

  $ hg parents --template "{rev}:{node|short}\n" -r 0
  $ hg log --template "{rev}:{node|short}\n" -r 0
  0:0cd96de13884

  $ get-with-headers.py localhost:$HGPORT 'comparison/0/a'
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
  <a href="https://mercurial-scm.org/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/0">log</a></li>
  <li><a href="/graph/0">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/0">changeset</a></li>
  <li><a href="/file/0">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/0/a">file</a></li>
  <li><a href="/file/tip/a">latest</a></li>
  <li><a href="/diff/0/a">diff</a></li>
  <li class="active">comparison</li>
  <li><a href="/annotate/0/a">annotate</a></li>
  <li><a href="/log/0/a">file log</a></li>
  <li><a href="/raw-file/0/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>
   comparison a @ 0:<a href="/rev/0cd96de13884">0cd96de13884</a>
   
  </h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
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
        <th>0:0cd96de13884</th>
      </tr>
    </thead>
    
  <tbody class="block">
  
  <tr id="r1">
  <td class="source insert"><a href="#r1">      </a> </td>
  <td class="source insert"><a href="#r1">     1</a> a</td>
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
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a >> a
  $ hg ci -mc

  $ hg parents --template "{rev}:{node|short}\n" -r tip
  1:559edbd9ed20
  $ hg log --template "{rev}:{node|short}\n" -r tip
  2:d73db4d812ff

  $ get-with-headers.py localhost:$HGPORT 'comparison/tip/a'
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
  <a href="https://mercurial-scm.org/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/tip">log</a></li>
  <li><a href="/graph/tip">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/tip">changeset</a></li>
  <li><a href="/file/tip">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/tip/a">file</a></li>
  <li><a href="/file/tip/a">latest</a></li>
  <li><a href="/diff/tip/a">diff</a></li>
  <li class="active">comparison</li>
  <li><a href="/annotate/tip/a">annotate</a></li>
  <li><a href="/log/tip/a">file log</a></li>
  <li><a href="/raw-file/tip/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>
   comparison a @ 2:<a href="/rev/d73db4d812ff">d73db4d812ff</a>
   <span class="tag">tip</span> 
  </h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
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
        <th>1:559edbd9ed20</th>
        <th>2:d73db4d812ff</th>
      </tr>
    </thead>
    
  <tbody class="block">
  
  <tr id="l1r1">
  <td class="source equal"><a href="#l1r1">     1</a> a</td>
  <td class="source equal"><a href="#l1r1">     1</a> a</td>
  </tr>
  <tr id="r2">
  <td class="source insert"><a href="#r2">      </a> </td>
  <td class="source insert"><a href="#r2">     2</a> a</td>
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

  $ hg parents --template "{rev}:{node|short}\n" -r tip
  2:d73db4d812ff
  $ hg log --template "{rev}:{node|short}\n" -r tip
  3:20e80271eb7a

  $ get-with-headers.py localhost:$HGPORT 'comparison/tip/a'
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
  <a href="https://mercurial-scm.org/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/tip">log</a></li>
  <li><a href="/graph/tip">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/tip">changeset</a></li>
  <li><a href="/file/tip">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/tip/a">file</a></li>
  <li><a href="/file/tip/a">latest</a></li>
  <li><a href="/diff/tip/a">diff</a></li>
  <li class="active">comparison</li>
  <li><a href="/annotate/tip/a">annotate</a></li>
  <li><a href="/log/tip/a">file log</a></li>
  <li><a href="/raw-file/tip/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>
   comparison a @ 3:<a href="/rev/20e80271eb7a">20e80271eb7a</a>
   <span class="tag">tip</span> 
  </h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
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
   <td><a href="/file/d73db4d812ff/a">d73db4d812ff</a> </td>
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
        <th>2:d73db4d812ff</th>
        <th>3:20e80271eb7a</th>
      </tr>
    </thead>
    
  <tbody class="block">
  
  <tr id="l1">
  <td class="source delete"><a href="#l1">     1</a> a</td>
  <td class="source delete"><a href="#l1">      </a> </td>
  </tr>
  <tr id="l2">
  <td class="source delete"><a href="#l2">     2</a> a</td>
  <td class="source delete"><a href="#l2">      </a> </td>
  </tr>
  </tbody>
  </table>
  
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

comparison not-modified file

  $ echo e > e
  $ hg add e
  $ hg ci -m e
  $ echo f > f
  $ hg add f
  $ hg ci -m f
  $ hg tip --template "{rev}:{node|short}\n"
  5:41d9fc4a6ae1
  $ hg diff -c tip e
  $ hg parents --template "{rev}:{node|short}\n" -r tip
  4:402bea3b0976
  $ hg parents --template "{rev}:{node|short}\n" -r tip e
  4:402bea3b0976

  $ get-with-headers.py localhost:$HGPORT 'comparison/tip/e'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: e comparison</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="https://mercurial-scm.org/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/tip">log</a></li>
  <li><a href="/graph/tip">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/tip">changeset</a></li>
  <li><a href="/file/tip">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/tip/e">file</a></li>
  <li><a href="/file/tip/e">latest</a></li>
  <li><a href="/diff/tip/e">diff</a></li>
  <li class="active">comparison</li>
  <li><a href="/annotate/tip/e">annotate</a></li>
  <li><a href="/log/tip/e">file log</a></li>
  <li><a href="/raw-file/tip/e">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>
   comparison e @ 5:<a href="/rev/41d9fc4a6ae1">41d9fc4a6ae1</a>
   <span class="tag">tip</span> 
  </h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <div class="description">f</div>
  
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
   <td><a href="/file/402bea3b0976/e">402bea3b0976</a> </td>
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
        <th>4:402bea3b0976</th>
        <th>5:41d9fc4a6ae1</th>
      </tr>
    </thead>
    
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

  $ killdaemons.py
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
  $ get-with-headers.py localhost:$HGPORT 'raw-rev/0'
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
  
  $ killdaemons.py
  $ rm .hg/hgrc rawdiff/map
  $ rmdir rawdiff
  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

errors

  $ cat ../test/errors.log

  $ cd ..
