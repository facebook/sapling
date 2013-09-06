  $ "$TESTDIR/hghave" serve || exit 80

setting up repo

  $ hg init test
  $ cd test
  $ echo a > a
  $ hg ci -Ama
  adding a
  $ hg rm a
  $ hg ci -mdel

set up hgweb

  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

revision

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'rev/tip'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: c78f6c5cbea9</title>
  </head>
  <body>
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
   <li><a href="/shortlog/c78f6c5cbea9">log</a></li>
   <li><a href="/graph/c78f6c5cbea9">graph</a></li>
   <li><a href="/tags">tags</a></li>
   <li><a href="/bookmarks">bookmarks</a></li>
   <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
   <li class="active">changeset</li>
   <li><a href="/raw-rev/c78f6c5cbea9">raw</a></li>
   <li><a href="/file/c78f6c5cbea9">browse</a></li>
  </ul>
  <ul>
   
  </ul>
  <ul>
   <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>changeset 1:c78f6c5cbea9  <span class="tag">tip</span>  </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <div class="description">del</div>
  
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
   <td class="author"><a href="/rev/cb9a9f314b8b">cb9a9f314b8b</a> </td>
  </tr>
  <tr>
   <th class="author">children</th>
   <td class="author"></td>
  </tr>
  <tr>
   <th class="files">files</th>
   <td class="files">a </td>
  </tr>
  <tr>
    <th class="diffstat">diffstat</th>
    <td class="diffstat">
       1 files changed, 0 insertions(+), 1 deletions(-)
  
      <a id="diffstatexpand" href="javascript:toggleDiffstat()"/>[<tt>+</tt>]</a>
      <div id="diffstatdetails" style="display:none;">
        <a href="javascript:toggleDiffstat()"/>[<tt>-</tt>]</a>
        <p>
        <table class="stripes2">  <tr>
      <td class="diffstat-file"><a href="#l1.1">a</a></td>
      <td class="diffstat-total" align="right">1</td>
      <td class="diffstat-graph">
        <span class="diffstat-add" style="width:0.0%;">&nbsp;</span>
        <span class="diffstat-remove" style="width:100.0%;">&nbsp;</span>
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
  <span id="l1.1" class="minusline">--- a/a	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l1.1"></a>
  <span id="l1.2" class="plusline">+++ /dev/null	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l1.2"></a>
  <span id="l1.3" class="atline">@@ -1,1 +0,0 @@</span><a href="#l1.3"></a>
  <span id="l1.4" class="minusline">-a</span><a href="#l1.4"></a></pre></div>
  </div>
  </div>
  
  </div>
  </div>
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

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
  <li><a href="/shortlog/c78f6c5cbea9">log</a></li>
  <li><a href="/graph/c78f6c5cbea9">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/c78f6c5cbea9">changeset</a></li>
  <li><a href="/file/c78f6c5cbea9">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/c78f6c5cbea9/a">file</a></li>
  <li><a href="/file/tip/a">latest</a></li>
  <li class="active">diff</li>
  <li><a href="/comparison/c78f6c5cbea9/a">comparison</a></li>
  <li><a href="/annotate/c78f6c5cbea9/a">annotate</a></li>
  <li><a href="/log/c78f6c5cbea9/a">file log</a></li>
  <li><a href="/raw-file/c78f6c5cbea9/a">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>diff a @ 1:c78f6c5cbea9</h3>
  
  <form class="search" action="/log">
  <p></p>
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <div class="description">del</div>
  
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
   <td><a href="/file/cb9a9f314b8b/a">cb9a9f314b8b</a> </td>
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
  <span id="l1.1" class="minusline">--- a/a	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l1.1"></a>
  <span id="l1.2" class="plusline">+++ /dev/null	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l1.2"></a>
  <span id="l1.3" class="atline">@@ -1,1 +0,0 @@</span><a href="#l1.3"></a>
  <span id="l1.4" class="minusline">-a</span><a href="#l1.4"></a></pre></div>
  </div>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

  $ cd ..
