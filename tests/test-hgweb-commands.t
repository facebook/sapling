An attempt at more fully testing the hgweb web interface.
The following things are tested elsewhere and are therefore omitted:
- archive, tested in test-archive
- unbundle, tested in test-push-http
- changegroupsubset, tested in test-pull

Set up the repo

  $ hg init test
  $ cd test
  $ mkdir da
  $ echo foo > da/foo
  $ echo foo > foo
  $ hg ci -Ambase
  adding da/foo
  adding foo
  $ hg tag 1.0
  $ hg bookmark something
  $ hg bookmark -r0 anotherthing
  $ echo another > foo
  $ hg branch stable
  marked working directory as branch stable
  $ hg ci -Ambranch
  $ hg serve --config server.uncompressed=False -n test -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

Logs and changes

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/log/?style=atom'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <feed xmlns="http://www.w3.org/2005/Atom">
   <!-- Changelog -->
   <id>http://*:$HGPORT/</id> (glob)
   <link rel="self" href="http://*:$HGPORT/atom-log"/> (glob)
   <link rel="alternate" href="http://*:$HGPORT/"/> (glob)
   <title>test Changelog</title>
   <updated>1970-01-01T00:00:00+00:00</updated>
  
   <entry>
    <title>branch</title>
    <id>http://*:$HGPORT/#changeset-1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe</id> (glob)
    <link href="http://*:$HGPORT/rev/1d22e65f027e"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
     <div xmlns="http://www.w3.org/1999/xhtml">
      <pre xml:space="preserve">branch</pre>
     </div>
    </content>
   </entry>
   <entry>
    <title>Added tag 1.0 for changeset 2ef0ac749a14</title>
    <id>http://*:$HGPORT/#changeset-a4f92ed23982be056b9852de5dfe873eaac7f0de</id> (glob)
    <link href="http://*:$HGPORT/rev/a4f92ed23982"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
     <div xmlns="http://www.w3.org/1999/xhtml">
      <pre xml:space="preserve">Added tag 1.0 for changeset 2ef0ac749a14</pre>
     </div>
    </content>
   </entry>
   <entry>
    <title>base</title>
    <id>http://*:$HGPORT/#changeset-2ef0ac749a14e4f57a5a822464a0902c6f7f448f</id> (glob)
    <link href="http://*:$HGPORT/rev/2ef0ac749a14"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
     <div xmlns="http://www.w3.org/1999/xhtml">
      <pre xml:space="preserve">base</pre>
     </div>
    </content>
   </entry>
  
  </feed>
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/log/1/?style=atom'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <feed xmlns="http://www.w3.org/2005/Atom">
   <!-- Changelog -->
   <id>http://*:$HGPORT/</id> (glob)
   <link rel="self" href="http://*:$HGPORT/atom-log"/> (glob)
   <link rel="alternate" href="http://*:$HGPORT/"/> (glob)
   <title>test Changelog</title>
   <updated>1970-01-01T00:00:00+00:00</updated>
  
   <entry>
    <title>branch</title>
    <id>http://*:$HGPORT/#changeset-1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe</id> (glob)
    <link href="http://*:$HGPORT/rev/1d22e65f027e"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
     <div xmlns="http://www.w3.org/1999/xhtml">
      <pre xml:space="preserve">branch</pre>
     </div>
    </content>
   </entry>
   <entry>
    <title>Added tag 1.0 for changeset 2ef0ac749a14</title>
    <id>http://*:$HGPORT/#changeset-a4f92ed23982be056b9852de5dfe873eaac7f0de</id> (glob)
    <link href="http://*:$HGPORT/rev/a4f92ed23982"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
     <div xmlns="http://www.w3.org/1999/xhtml">
      <pre xml:space="preserve">Added tag 1.0 for changeset 2ef0ac749a14</pre>
     </div>
    </content>
   </entry>
   <entry>
    <title>base</title>
    <id>http://*:$HGPORT/#changeset-2ef0ac749a14e4f57a5a822464a0902c6f7f448f</id> (glob)
    <link href="http://*:$HGPORT/rev/2ef0ac749a14"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
     <div xmlns="http://www.w3.org/1999/xhtml">
      <pre xml:space="preserve">base</pre>
     </div>
    </content>
   </entry>
  
  </feed>
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/log/1/foo/?style=atom'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <feed xmlns="http://www.w3.org/2005/Atom">
   <id>http://*:$HGPORT/atom-log/tip/foo</id> (glob)
   <link rel="self" href="http://*:$HGPORT/atom-log/tip/foo"/> (glob)
   <title>test: foo history</title>
   <updated>1970-01-01T00:00:00+00:00</updated>
  
   <entry>
    <title>base</title>
    <id>http://*:$HGPORT/#changeset-2ef0ac749a14e4f57a5a822464a0902c6f7f448f</id> (glob)
    <link href="http://*:$HGPORT/rev/2ef0ac749a14"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
     <div xmlns="http://www.w3.org/1999/xhtml">
      <pre xml:space="preserve">base</pre>
     </div>
    </content>
   </entry>
  
  </feed>
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/shortlog/'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: log</title>
  <link rel="alternate" type="application/atom+xml"
     href="/atom-log" title="Atom feed for test" />
  <link rel="alternate" type="application/rss+xml"
     href="/rss-log" title="RSS feed for test" />
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li class="active">log</li>
  <li><a href="/graph/1d22e65f027e">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/1d22e65f027e">changeset</a></li>
  <li><a href="/file/1d22e65f027e">browse</a></li>
  </ul>
  <ul>
  
  </ul>
  <ul>
   <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>log</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="navigate">
  <a href="/shortlog/2?revcount=30">less</a>
  <a href="/shortlog/2?revcount=120">more</a>
  | rev 2: <a href="/shortlog/2ef0ac749a14">(0)</a> <a href="/shortlog/tip">tip</a> 
  </div>
  
  <table class="bigtable">
   <tr>
    <th class="age">age</th>
    <th class="author">author</th>
    <th class="description">description</th>
   </tr>
   <tr class="parity0">
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/1d22e65f027e">branch</a><span class="branchhead">stable</span> <span class="tag">tip</span> <span class="tag">something</span> </td>
   </tr>
   <tr class="parity1">
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/a4f92ed23982">Added tag 1.0 for changeset 2ef0ac749a14</a><span class="branchhead">default</span> </td>
   </tr>
   <tr class="parity0">
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/2ef0ac749a14">base</a><span class="tag">1.0</span> <span class="tag">anotherthing</span> </td>
   </tr>
  
  </table>
  
  <div class="navigate">
  <a href="/shortlog/2?revcount=30">less</a>
  <a href="/shortlog/2?revcount=120">more</a>
  | rev 2: <a href="/shortlog/2ef0ac749a14">(0)</a> <a href="/shortlog/tip">tip</a> 
  </div>
  
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/rev/0/'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: 2ef0ac749a14</title>
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
   <li class="active">changeset</li>
   <li><a href="/raw-rev/2ef0ac749a14">raw</a></li>
   <li><a href="/file/2ef0ac749a14">browse</a></li>
  </ul>
  <ul>
   
  </ul>
  <ul>
   <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  
  <h2><a href="/">test</a></h2>
  <h3>changeset 0:2ef0ac749a14  <span class="tag">1.0</span>  <span class="tag">anotherthing</span> </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="description">base</div>
  
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
   <td class="author"> <a href="/rev/a4f92ed23982">a4f92ed23982</a></td>
  </tr>
  <tr>
   <th class="files">files</th>
   <td class="files"><a href="/file/2ef0ac749a14/da/foo">da/foo</a> <a href="/file/2ef0ac749a14/foo">foo</a> </td>
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
      <td class="diffstat-file"><a href="#l1.1">da/foo</a></td>
      <td class="diffstat-total" align="right">1</td>
      <td class="diffstat-graph">
        <span class="diffstat-add" style="width:100.0%;">&nbsp;</span>
        <span class="diffstat-remove" style="width:0.0%;">&nbsp;</span>
      </td>
    </tr>
    <tr class="parity1">
      <td class="diffstat-file"><a href="#l2.1">foo</a></td>
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
  </span><a href="#l1.2" id="l1.2">     1.2</a> <span class="plusline">+++ b/da/foo	Thu Jan 01 00:00:00 1970 +0000
  </span><a href="#l1.3" id="l1.3">     1.3</a> <span class="atline">@@ -0,0 +1,1 @@
  </span><a href="#l1.4" id="l1.4">     1.4</a> <span class="plusline">+foo
  </span></pre></div><div class="source bottomline parity1"><pre><a href="#l2.1" id="l2.1">     2.1</a> <span class="minusline">--- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  </span><a href="#l2.2" id="l2.2">     2.2</a> <span class="plusline">+++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  </span><a href="#l2.3" id="l2.3">     2.3</a> <span class="atline">@@ -0,0 +1,1 @@
  </span><a href="#l2.4" id="l2.4">     2.4</a> <span class="plusline">+foo
  </span></pre></div>
  </div>
  
  </div>
  </div>
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/rev/1/?style=raw'
  200 Script output follows
  
  
  # HG changeset patch
  # User test
  # Date 0 0
  # Node ID a4f92ed23982be056b9852de5dfe873eaac7f0de
  # Parent  2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  Added tag 1.0 for changeset 2ef0ac749a14
  
  diff -r 2ef0ac749a14 -r a4f92ed23982 .hgtags
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +2ef0ac749a14e4f57a5a822464a0902c6f7f448f 1.0
  
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/log?rev=base'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: searching for base</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" width=75 height=90 border=0 alt="mercurial"></a>
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
  <h3>searching for 'base'</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30"></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="navigate">
  <a href="/search/?rev=base&revcount=5">less</a>
  <a href="/search/?rev=base&revcount=20">more</a>
  </div>
  
  <table class="bigtable">
   <tr>
    <th class="age">age</th>
    <th class="author">author</th>
    <th class="description">description</th>
   </tr>
   <tr class="parity0">
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/2ef0ac749a14">base</a><span class="tag">1.0</span> <span class="tag">anotherthing</span> </td>
   </tr>
  
  </table>
  
  <div class="navigate">
  <a href="/search/?rev=base&revcount=5">less</a>
  <a href="/search/?rev=base&revcount=20">more</a>
  </div>
  
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

File-related

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/file/1/foo/?style=raw'
  200 Script output follows
  
  foo
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/annotate/1/foo/?style=raw'
  200 Script output follows
  
  
  test@0: foo
  
  
  
  
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/file/1/?style=raw'
  200 Script output follows
  
  
  drwxr-xr-x da
  -rw-r--r-- 45 .hgtags
  -rw-r--r-- 4 foo
  
  
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/file/1/foo'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: a4f92ed23982 foo</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/a4f92ed23982">log</a></li>
  <li><a href="/graph/a4f92ed23982">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/a4f92ed23982">changeset</a></li>
  <li><a href="/file/a4f92ed23982/">browse</a></li>
  </ul>
  <ul>
  <li class="active">file</li>
  <li><a href="/file/tip/foo">latest</a></li>
  <li><a href="/diff/a4f92ed23982/foo">diff</a></li>
  <li><a href="/annotate/a4f92ed23982/foo">annotate</a></li>
  <li><a href="/log/a4f92ed23982/foo">file log</a></li>
  <li><a href="/raw-file/a4f92ed23982/foo">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>view foo @ 1:a4f92ed23982</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="description">Added tag 1.0 for changeset 2ef0ac749a14</div>
  
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
   <td class="author"><a href="/file/1d22e65f027e/foo">1d22e65f027e</a> </td>
  </tr>
  
  </table>
  
  <div class="overflow">
  <div class="sourcefirst"> line source</div>
  
  <div class="parity0 source"><a href="#l1" id="l1">     1</a> foo
  </div>
  <div class="sourcelast"></div>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/filediff/1/foo/?style=raw'
  200 Script output follows
  
  
  diff -r 000000000000 -r a4f92ed23982 foo
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +foo
  
  
  
  

Overviews

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/raw-tags'
  200 Script output follows
  
  tip	1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe
  1.0	2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/raw-branches'
  200 Script output follows
  
  stable	1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe	open
  default	a4f92ed23982be056b9852de5dfe873eaac7f0de	inactive
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/raw-bookmarks'
  200 Script output follows
  
  anotherthing	2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  something	1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/summary/?style=gitweb'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US" lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow"/>
  <link rel="stylesheet" href="/static/style-gitweb.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: Summary</title>
  <link rel="alternate" type="application/atom+xml"
     href="/atom-log" title="Atom feed for test"/>
  <link rel="alternate" type="application/rss+xml"
     href="/rss-log" title="RSS feed for test"/>
  </head>
  <body>
  
  <div class="page_header">
  <a href="http://mercurial.selenic.com/" title="Mercurial" style="float: right;">Mercurial</a><a href="/summary?style=gitweb">test</a> / summary
  
  <form action="/log">
  <input type="hidden" name="style" value="gitweb" />
  <div class="search">
  <input type="text" name="rev"  />
  </div>
  </form>
  </div>
  
  <div class="page_nav">
  summary |
  <a href="/shortlog?style=gitweb">shortlog</a> |
  <a href="/log?style=gitweb">changelog</a> |
  <a href="/graph?style=gitweb">graph</a> |
  <a href="/tags?style=gitweb">tags</a> |
  <a href="/bookmarks?style=gitweb">bookmarks</a> |
  <a href="/branches?style=gitweb">branches</a> |
  <a href="/file/1d22e65f027e?style=gitweb">files</a> |
  <a href="/help?style=gitweb">help</a>
  <br/>
  </div>
  
  <div class="title">&nbsp;</div>
  <table cellspacing="0">
  <tr><td>description</td><td>unknown</td></tr>
  <tr><td>owner</td><td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td></tr>
  <tr><td>last change</td><td>Thu, 01 Jan 1970 00:00:00 +0000</td></tr>
  </table>
  
  <div><a  class="title" href="/shortlog?style=gitweb">changes</a></div>
  <table cellspacing="0">
  
  <tr class="parity0">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><i>test</i></td>
  <td>
  <a class="list" href="/rev/1d22e65f027e?style=gitweb">
  <b>branch</b>
  <span class="logtags"><span class="branchtag" title="stable">stable</span> <span class="tagtag" title="tip">tip</span> <span class="bookmarktag" title="something">something</span> </span>
  </a>
  </td>
  <td class="link" nowrap>
  <a href="/rev/1d22e65f027e?style=gitweb">changeset</a> |
  <a href="/file/1d22e65f027e?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity1">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><i>test</i></td>
  <td>
  <a class="list" href="/rev/a4f92ed23982?style=gitweb">
  <b>Added tag 1.0 for changeset 2ef0ac749a14</b>
  <span class="logtags"><span class="branchtag" title="default">default</span> </span>
  </a>
  </td>
  <td class="link" nowrap>
  <a href="/rev/a4f92ed23982?style=gitweb">changeset</a> |
  <a href="/file/a4f92ed23982?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity0">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><i>test</i></td>
  <td>
  <a class="list" href="/rev/2ef0ac749a14?style=gitweb">
  <b>base</b>
  <span class="logtags"><span class="tagtag" title="1.0">1.0</span> <span class="bookmarktag" title="anotherthing">anotherthing</span> </span>
  </a>
  </td>
  <td class="link" nowrap>
  <a href="/rev/2ef0ac749a14?style=gitweb">changeset</a> |
  <a href="/file/2ef0ac749a14?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="light"><td colspan="4"><a class="list" href="/shortlog?style=gitweb">...</a></td></tr>
  </table>
  
  <div><a class="title" href="/tags?style=gitweb">tags</a></div>
  <table cellspacing="0">
  
  <tr class="parity0">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><a class="list" href="/rev/2ef0ac749a14?style=gitweb"><b>1.0</b></a></td>
  <td class="link">
  <a href="/rev/2ef0ac749a14?style=gitweb">changeset</a> |
  <a href="/log/2ef0ac749a14?style=gitweb">changelog</a> |
  <a href="/file/2ef0ac749a14?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="light"><td colspan="3"><a class="list" href="/tags?style=gitweb">...</a></td></tr>
  </table>
  
  <div><a class="title" href="/bookmarks?style=gitweb">bookmarks</a></div>
  <table cellspacing="0">
  
  <tr class="parity0">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><a class="list" href="/rev/2ef0ac749a14?style=gitweb"><b>anotherthing</b></a></td>
  <td class="link">
  <a href="/rev/2ef0ac749a14?style=gitweb">changeset</a> |
  <a href="/log/2ef0ac749a14?style=gitweb">changelog</a> |
  <a href="/file/2ef0ac749a14?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity1">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><a class="list" href="/rev/1d22e65f027e?style=gitweb"><b>something</b></a></td>
  <td class="link">
  <a href="/rev/1d22e65f027e?style=gitweb">changeset</a> |
  <a href="/log/1d22e65f027e?style=gitweb">changelog</a> |
  <a href="/file/1d22e65f027e?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="light"><td colspan="3"><a class="list" href="/bookmarks?style=gitweb">...</a></td></tr>
  </table>
  
  <div><a class="title" href="#">branches</a></div>
  <table cellspacing="0">
  
  <tr class="parity0">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><a class="list" href="/shortlog/1d22e65f027e?style=gitweb"><b>1d22e65f027e</b></a></td>
  <td class="">stable</td>
  <td class="link">
  <a href="/changeset/1d22e65f027e?style=gitweb">changeset</a> |
  <a href="/log/1d22e65f027e?style=gitweb">changelog</a> |
  <a href="/file/1d22e65f027e?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity1">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><a class="list" href="/shortlog/a4f92ed23982?style=gitweb"><b>a4f92ed23982</b></a></td>
  <td class="">default</td>
  <td class="link">
  <a href="/changeset/a4f92ed23982?style=gitweb">changeset</a> |
  <a href="/log/a4f92ed23982?style=gitweb">changelog</a> |
  <a href="/file/a4f92ed23982?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="light">
    <td colspan="4"><a class="list"  href="#">...</a></td>
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
  
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/graph/?style=gitweb'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US" lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow"/>
  <link rel="stylesheet" href="/static/style-gitweb.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: Graph</title>
  <link rel="alternate" type="application/atom+xml"
     href="/atom-log" title="Atom feed for test"/>
  <link rel="alternate" type="application/rss+xml"
     href="/rss-log" title="RSS feed for test"/>
  <!--[if IE]><script type="text/javascript" src="/static/excanvas.js"></script><![endif]-->
  </head>
  <body>
  
  <div class="page_header">
  <a href="http://mercurial.selenic.com/" title="Mercurial" style="float: right;">Mercurial</a><a href="/summary?style=gitweb">test</a> / graph
  </div>
  
  <form action="/log">
  <input type="hidden" name="style" value="gitweb" />
  <div class="search">
  <input type="text" name="rev"  />
  </div>
  </form>
  <div class="page_nav">
  <a href="/summary?style=gitweb">summary</a> |
  <a href="/shortlog?style=gitweb">shortlog</a> |
  <a href="/log/2?style=gitweb">changelog</a> |
  graph |
  <a href="/tags?style=gitweb">tags</a> |
  <a href="/bookmarks?style=gitweb">bookmarks</a> |
  <a href="/branches?style=gitweb">branches</a> |
  <a href="/file/1d22e65f027e?style=gitweb">files</a> |
  <a href="/help?style=gitweb">help</a>
  <br/>
  <a href="/graph/2?style=gitweb&revcount=30">less</a>
  <a href="/graph/2?style=gitweb&revcount=120">more</a>
  | <a href="/graph/2ef0ac749a14?style=gitweb">(0)</a> <a href="/graph/2ef0ac749a14?style=gitweb">-2</a> <a href="/graph/tip?style=gitweb">tip</a> <br/>
  </div>
  
  <div class="title">&nbsp;</div>
  
  <noscript>The revision graph only works with JavaScript-enabled browsers.</noscript>
  
  <div id="wrapper">
  <ul id="nodebgs"></ul>
  <canvas id="graph" width="480" height="129"></canvas>
  <ul id="graphnodes"></ul>
  </div>
  
  <script>
  <!-- hide script content
  
  var data = [["1d22e65f027e", [0, 1], [[0, 0, 1]], "branch", "test", "1970-01-01", ["stable", true], ["tip"], ["something"]], ["a4f92ed23982", [0, 1], [[0, 0, 1]], "Added tag 1.0 for changeset 2ef0ac749a14", "test", "1970-01-01", ["default", true], [], []], ["2ef0ac749a14", [0, 1], [], "base", "test", "1970-01-01", ["default", false], ["1.0"], ["anotherthing"]]];
  var graph = new Graph();
  graph.scale(39);
  
  graph.edge = function(x0, y0, x1, y1, color) {
  	
  	this.setColor(color, 0.0, 0.65);
  	this.ctx.beginPath();
  	this.ctx.moveTo(x0, y0);
  	this.ctx.lineTo(x1, y1);
  	this.ctx.stroke();
  	
  }
  
  var revlink = '<li style="_STYLE"><span class="desc">';
  revlink += '<a class="list" href="/rev/_NODEID?style=gitweb" title="_NODEID"><b>_DESC</b></a>';
  revlink += '</span> _TAGS';
  revlink += '<span class="info">_DATE, by _USER</span></li>';
  
  graph.vertex = function(x, y, color, parity, cur) {
  	
  	this.ctx.beginPath();
  	color = this.setColor(color, 0.25, 0.75);
  	this.ctx.arc(x, y, radius, 0, Math.PI * 2, true);
  	this.ctx.fill();
  	
  	var bg = '<li class="bg parity' + parity + '"></li>';
  	var left = (this.columns + 1) * this.bg_height;
  	var nstyle = 'padding-left: ' + left + 'px;';
  	var item = revlink.replace(/_STYLE/, nstyle);
  	item = item.replace(/_PARITY/, 'parity' + parity);
  	item = item.replace(/_NODEID/, cur[0]);
  	item = item.replace(/_NODEID/, cur[0]);
  	item = item.replace(/_DESC/, cur[3]);
  	item = item.replace(/_USER/, cur[4]);
  	item = item.replace(/_DATE/, cur[5]);
  	
  	var tagspan = '';
  	if (cur[7].length || cur[8].length || (cur[6][0] != 'default' || cur[6][1])) {
  		tagspan = '<span class="logtags">';
  		if (cur[6][1]) {
  			tagspan += '<span class="branchtag" title="' + cur[6][0] + '">';
  			tagspan += cur[6][0] + '</span> ';
  		} else if (!cur[6][1] && cur[6][0] != 'default') {
  			tagspan += '<span class="inbranchtag" title="' + cur[6][0] + '">';
  			tagspan += cur[6][0] + '</span> ';
  		}
  		if (cur[7].length) {
  			for (var t in cur[7]) {
  				var tag = cur[7][t];
  				tagspan += '<span class="tagtag">' + tag + '</span> ';
  			}
  		}
  		if (cur[8].length) {
  			for (var t in cur[8]) {
  				var bookmark = cur[8][t];
  				tagspan += '<span class="bookmarktag">' + bookmark + '</span> ';
  			}
  		}
  		tagspan += '</span>';
  	}
  	
  	item = item.replace(/_TAGS/, tagspan);
  	return [bg, item];
  	
  }
  
  graph.render(data);
  
  // stop hiding script -->
  </script>
  
  <div class="page_nav">
  <a href="/graph/2?style=gitweb&revcount=30">less</a>
  <a href="/graph/2?style=gitweb&revcount=120">more</a>
  | <a href="/graph/2ef0ac749a14?style=gitweb">(0)</a> <a href="/graph/2ef0ac749a14?style=gitweb">-2</a> <a href="/graph/tip?style=gitweb">tip</a> 
  </div>
  
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
  

capabilities

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '?cmd=capabilities'; echo
  200 Script output follows
  
  lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024

heads

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '?cmd=heads'
  200 Script output follows
  
  1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe

branches

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '?cmd=branches&nodes=0000000000000000000000000000000000000000'
  200 Script output follows
  
  0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000

changegroup

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '?cmd=changegroup&roots=0000000000000000000000000000000000000000'
  200 Script output follows
  
  x\x9c\xbdTMHUA\x14\xbe\xa8\xf9\xec\xda&\x10\x11*\xb8\x88\x81\x99\xbef\xe6\xce\xbdw\xc6\xf2a\x16E\x1b\x11[%\x98\xcc\xaf\x8f\x8c\xf7\xc0\xf7\x82 (esc)
  4\x11KP2m\x95\xad*\xabE\x05AP\xd0\xc22Z\x14\xf9\x03\xb9j\xa3\x9b$\xa4MJ\xb4\x90\xc0\x9a\x9bO0\x10\xdf\x13\xa2\x81\x0f\x869g\xe6|\xe7\x9c\xef\x8ceY\xf7\xa2KO\xd2\xb7K\x16~\\n\xe9\xad\x90w\x86\xab\x93W\x8e\xdf\xb0r\\Y\xee6(\xa2)\xf6\x95\xc6\x01\xe4\x1az\x80R\xe8kN\x98\xe7R\xa4\xa9K@\xe0!A\xb4k\xa7U*m\x03\x07\xd8\x92\x1d\xd2\xc9\xa4\x1d\xc2\xe6,\xa5\xcc+\x1f\xef\xafDgi\xef\xab\x1d\x1d\xb7\x9a\xe7[W\xfbc\x8f\xde-\xcd\xe7\xcaz\xb3\xbb\x19\xd3\x81\x10>c>\x08\x00"X\x11\xc2\x84@\xd2\xe7B*L\x00\x01P\x04R\xc3@\xbaB0\xdb8#\x83:\x83\xa2h\xbc=\xcd\xdaS\xe1Y,L\xd3\xa0\xf2\xa8\x94J:\xe6\xd8\x81Q\xe0\xe8d\xa7#\xe2,\xd1\xaeR*\xed \xa5\x01\x13\x01\xa6\x0cb\xe3;\xbe\xaf\xfcK[^wK\xe1N\xaf\xbbk\xe8B\xd1\xf4\xc1\x07\xb3\xab[\x10\xfdkmvwcB\xa6\xa4\xd4G\xc4D\xc2\x141\xad\x91\x10\x00\x08J\x81\xcb}\xee	\xee+W\xba\x8a\x80\x90|\xd4\xa0\xd6\xa0\xd4T\xde\xe1\x9d,!\xe2\xb5\xa94\xe3\xe7\xd5\x9f\x06\x18\xcba\x03aP\xb8f\xcd\x04\x1a_\\9\xf1\xed\xe4\x9e\xe5\xa6\xd1\xd2\x9f\x03\xa7o\xae\x90H\xf3\xfb\xef\xffH3\xadk (esc)
  \xb0\x90\x92\x88\xb9\x14"\x068\xc2\x1e@\x00\xbb\x8a)\xd3'\x859 (esc)
  \xa8\x80\x84S \xa5\xbd-g\x13`\xe4\xdc\xc3H^\xdf\xe2\xc0TM\xc7\xf4BO\xcf\xde\xae\xe5\xae#\x1frM(K\x97`F\x19\x16s\x05GD\xb9\x01\xc1\x00+\x8c|\x9fp\xc11\xf0\x14\x00\x9cJ\x82<\xe0\x12\x9f\xc1\x90\xd0\xf5\xc8\x19>Pr\xaa\xeaW\xf5\xc4\xae\xd1\xfc\x17\xcf'\x13u\xb1\x9e\xcdHnC\x0e\xcc`\xc8\xa0&\xac\x0e\xf1|\x8c\x10$\xc4\x8c\xa2p\x05`\xdc\x08 \x80\xc4\xd7Rr-\x94\x10\x102\xedi;\xf3f\xf1z\x16\x86\xdb\xd8d\xe5\xe7\x8b\xf5\x8d\rzp\xb2\xfe\xac\xf5\xf2\xd3\xfe\xfckws\xedt\x96b\xd5l\x1c\x0b\x85\xb5\x170\x8f\x11\x84\xb0\x8f\x19\xa0\x00	_\x07\x1ac\xa2\xc3\x89Z\xe7\x96\xf9 \xccNFg\xc7F\xaa\x8a+\x9a\x9cc_\x17\x1b\x17\x9e]z38<\x97+\xb5,",\xc8\xc8?\\\x91\xff\x17.~U\x96\x97\xf5%\xdeN<\x8e\xf5\x97%\xe7^\xcfL\xed~\xda\x96k\xdc->\x86\x02\x83"\x96H\xa6\xe3\xaas=-\xeb7\xe5\xda\x8f\xbc (no-eol) (esc)

stream_out

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '?cmd=stream_out'
  200 Script output follows
  
  1

failing unbundle, requires POST request

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '?cmd=unbundle'
  405 push requires POST request
  
  0
  push requires POST request
  [1]

Static files

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/static/style.css'
  200 Script output follows
  
  a { text-decoration:none; }
  .age { white-space:nowrap; }
  .date { white-space:nowrap; }
  .indexlinks { white-space:nowrap; }
  .parity0 { background-color: #ddd; }
  .parity1 { background-color: #eee; }
  .lineno { width: 60px; color: #aaa; font-size: smaller;
            text-align: right; }
  .plusline { color: green; }
  .minusline { color: red; }
  .atline { color: purple; }
  .annotate { font-size: smaller; text-align: right; padding-right: 1em; }
  .buttons a {
    background-color: #666;
    padding: 2pt;
    color: white;
    font-family: sans;
    font-weight: bold;
  }
  .navigate a {
    background-color: #ccc;
    padding: 2pt;
    font-family: sans;
    color: black;
  }
  
  .metatag {
    background-color: #888;
    color: white;
    text-align: right;
  }
  
  /* Common */
  pre { margin: 0; }
  
  .logo {
    float: right;
    clear: right;
  }
  
  /* Changelog/Filelog entries */
  .logEntry { width: 100%; }
  .logEntry .age { width: 15%; }
  .logEntry th { font-weight: normal; text-align: right; vertical-align: top; }
  .logEntry th.age, .logEntry th.firstline { font-weight: bold; }
  .logEntry th.firstline { text-align: left; width: inherit; }
  
  /* Shortlog entries */
  .slogEntry { width: 100%; }
  .slogEntry .age { width: 8em; }
  .slogEntry td { font-weight: normal; text-align: left; vertical-align: top; }
  .slogEntry td.author { width: 15em; }
  
  /* Tag entries */
  #tagEntries { list-style: none; margin: 0; padding: 0; }
  #tagEntries .tagEntry { list-style: none; margin: 0; padding: 0; }
  
  /* Changeset entry */
  #changesetEntry { }
  #changesetEntry th { font-weight: normal; background-color: #888; color: #fff; text-align: right; }
  #changesetEntry th.files, #changesetEntry th.description { vertical-align: top; }
  
  /* File diff view */
  #filediffEntry { }
  #filediffEntry th { font-weight: normal; background-color: #888; color: #fff; text-align: right; }
  
  /* Graph */
  div#wrapper {
  	position: relative;
  	margin: 0;
  	padding: 0;
  }
  
  canvas {
  	position: absolute;
  	z-index: 5;
  	top: -0.6em;
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
  	top: -0.85em;
  	list-style: none inside none;
  	padding: 0;
  }
  
  ul#graphnodes li .info {
  	display: block;
  	font-size: 70%;
  	position: relative;
  	top: -1px;
  }

Stop and restart with HGENCODING=cp932

  $ "$TESTDIR/killdaemons.py"
  $ HGENCODING=cp932 hg serve --config server.uncompressed=False -n test \
  >     -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

commit message with Japanese Kanji 'Noh', which ends with '\x5c'

  $ echo foo >> foo
  $ HGENCODING=cp932 hg ci -m `python -c 'print("\x94\x5c")'`

Graph json escape of multibyte character

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/graph/' \
  >     | grep '^var data ='
  var data = [["40b4d6888e92", [0, 1], [[0, 0, 1]], "\u80fd", "test", "1970-01-01", ["stable", true], ["tip"], ["something"]], ["1d22e65f027e", [0, 1], [[0, 0, 1]], "branch", "test", "1970-01-01", ["stable", false], [], []], ["a4f92ed23982", [0, 1], [[0, 0, 1]], "Added tag 1.0 for changeset 2ef0ac749a14", "test", "1970-01-01", ["default", true], [], []], ["2ef0ac749a14", [0, 1], [], "base", "test", "1970-01-01", ["default", false], ["1.0"], ["anotherthing"]]];

ERRORS ENCOUNTERED

  $ cat errors.log
