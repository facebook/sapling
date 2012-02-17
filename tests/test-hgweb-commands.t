  $ "$TESTDIR/hghave" serve || exit 80

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
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -Ambranch
  $ hg branch unstable
  marked working directory as branch unstable
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -Ambranch
  $ echo [graph] >> .hg/hgrc
  $ echo default.width = 3 >> .hg/hgrc
  $ echo stable.width = 3 >> .hg/hgrc
  $ echo stable.color = FF0000 >> .hg/hgrc
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
    <id>http://*:$HGPORT/#changeset-ba87b23d29ca67a305625d81a20ac279c1e3f444</id> (glob)
    <link href="http://*:$HGPORT/rev/ba87b23d29ca"/> (glob)
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
    <id>http://*:$HGPORT/#changeset-ba87b23d29ca67a305625d81a20ac279c1e3f444</id> (glob)
    <link href="http://*:$HGPORT/rev/ba87b23d29ca"/> (glob)
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
  <li><a href="/graph/ba87b23d29ca">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/ba87b23d29ca">changeset</a></li>
  <li><a href="/file/ba87b23d29ca">browse</a></li>
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
  <a href="/shortlog/3?revcount=30">less</a>
  <a href="/shortlog/3?revcount=120">more</a>
  | rev 3: <a href="/shortlog/2ef0ac749a14">(0)</a> <a href="/shortlog/tip">tip</a> 
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
    <td class="description"><a href="/rev/ba87b23d29ca">branch</a><span class="branchhead">unstable</span> <span class="tag">tip</span> <span class="tag">something</span> </td>
   </tr>
   <tr class="parity1">
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/1d22e65f027e">branch</a><span class="branchhead">stable</span> </td>
   </tr>
   <tr class="parity0">
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/a4f92ed23982">Added tag 1.0 for changeset 2ef0ac749a14</a><span class="branchhead">default</span> </td>
   </tr>
   <tr class="parity1">
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description"><a href="/rev/2ef0ac749a14">base</a><span class="tag">1.0</span> <span class="tag">anotherthing</span> </td>
   </tr>
  
  </table>
  
  <div class="navigate">
  <a href="/shortlog/3?revcount=30">less</a>
  <a href="/shortlog/3?revcount=120">more</a>
  | rev 3: <a href="/shortlog/2ef0ac749a14">(0)</a> <a href="/shortlog/tip">tip</a> 
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
  
  tip	ba87b23d29ca67a305625d81a20ac279c1e3f444
  1.0	2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/raw-branches'
  200 Script output follows
  
  unstable	ba87b23d29ca67a305625d81a20ac279c1e3f444	open
  stable	1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe	inactive
  default	a4f92ed23982be056b9852de5dfe873eaac7f0de	inactive
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '/raw-bookmarks'
  200 Script output follows
  
  anotherthing	2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  something	ba87b23d29ca67a305625d81a20ac279c1e3f444
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
  <a href="/file/ba87b23d29ca?style=gitweb">files</a> |
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
  <a class="list" href="/rev/ba87b23d29ca?style=gitweb">
  <b>branch</b>
  <span class="logtags"><span class="branchtag" title="unstable">unstable</span> <span class="tagtag" title="tip">tip</span> <span class="bookmarktag" title="something">something</span> </span>
  </a>
  </td>
  <td class="link" nowrap>
  <a href="/rev/ba87b23d29ca?style=gitweb">changeset</a> |
  <a href="/file/ba87b23d29ca?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity1">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><i>test</i></td>
  <td>
  <a class="list" href="/rev/1d22e65f027e?style=gitweb">
  <b>branch</b>
  <span class="logtags"><span class="branchtag" title="stable">stable</span> </span>
  </a>
  </td>
  <td class="link" nowrap>
  <a href="/rev/1d22e65f027e?style=gitweb">changeset</a> |
  <a href="/file/1d22e65f027e?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity0">
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
  <tr class="parity1">
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
  <td><a class="list" href="/rev/ba87b23d29ca?style=gitweb"><b>something</b></a></td>
  <td class="link">
  <a href="/rev/ba87b23d29ca?style=gitweb">changeset</a> |
  <a href="/log/ba87b23d29ca?style=gitweb">changelog</a> |
  <a href="/file/ba87b23d29ca?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="light"><td colspan="3"><a class="list" href="/bookmarks?style=gitweb">...</a></td></tr>
  </table>
  
  <div><a class="title" href="#">branches</a></div>
  <table cellspacing="0">
  
  <tr class="parity0">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><a class="list" href="/shortlog/ba87b23d29ca?style=gitweb"><b>ba87b23d29ca</b></a></td>
  <td class="">unstable</td>
  <td class="link">
  <a href="/changeset/ba87b23d29ca?style=gitweb">changeset</a> |
  <a href="/log/ba87b23d29ca?style=gitweb">changelog</a> |
  <a href="/file/ba87b23d29ca?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity1">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><a class="list" href="/shortlog/1d22e65f027e?style=gitweb"><b>1d22e65f027e</b></a></td>
  <td class="">stable</td>
  <td class="link">
  <a href="/changeset/1d22e65f027e?style=gitweb">changeset</a> |
  <a href="/log/1d22e65f027e?style=gitweb">changelog</a> |
  <a href="/file/1d22e65f027e?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity0">
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
  <a href="/log/3?style=gitweb">changelog</a> |
  graph |
  <a href="/tags?style=gitweb">tags</a> |
  <a href="/bookmarks?style=gitweb">bookmarks</a> |
  <a href="/branches?style=gitweb">branches</a> |
  <a href="/file/ba87b23d29ca?style=gitweb">files</a> |
  <a href="/help?style=gitweb">help</a>
  <br/>
  <a href="/graph/3?style=gitweb&revcount=30">less</a>
  <a href="/graph/3?style=gitweb&revcount=120">more</a>
  | <a href="/graph/2ef0ac749a14?style=gitweb">(0)</a> <a href="/graph/2ef0ac749a14?style=gitweb">-3</a> <a href="/graph/tip?style=gitweb">tip</a> <br/>
  </div>
  
  <div class="title">&nbsp;</div>
  
  <noscript>The revision graph only works with JavaScript-enabled browsers.</noscript>
  
  <div id="wrapper">
  <ul id="nodebgs"></ul>
  <canvas id="graph" width="480" height="168"></canvas>
  <ul id="graphnodes"></ul>
  </div>
  
  <script>
  <!-- hide script content
  
  var data = [["ba87b23d29ca", [0, 1], [[0, 0, 1, {"color": "FF0000", "width": "3"}]], "branch", "test", "1970-01-01", ["unstable", true], ["tip"], ["something"]], ["1d22e65f027e", [0, 1], [[0, 0, 1, {"width": "3"}]], "branch", "test", "1970-01-01", ["stable", true], [], []], ["a4f92ed23982", [0, 1], [[0, 0, 1, {"width": "3"}]], "Added tag 1.0 for changeset 2ef0ac749a14", "test", "1970-01-01", ["default", true], [], []], ["2ef0ac749a14", [0, 1], [], "base", "test", "1970-01-01", ["default", false], ["1.0"], ["anotherthing"]]];
  var graph = new Graph();
  graph.scale(39);
  
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
  <a href="/graph/3?style=gitweb&revcount=30">less</a>
  <a href="/graph/3?style=gitweb&revcount=120">more</a>
  | <a href="/graph/2ef0ac749a14?style=gitweb">(0)</a> <a href="/graph/2ef0ac749a14?style=gitweb">-3</a> <a href="/graph/tip?style=gitweb">tip</a> 
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
  
  ba87b23d29ca67a305625d81a20ac279c1e3f444

branches

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '?cmd=branches&nodes=0000000000000000000000000000000000000000'
  200 Script output follows
  
  0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000

changegroup

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT '?cmd=changegroup&roots=0000000000000000000000000000000000000000'
  200 Script output follows
  
  x\x9c\xbdTMHTQ\x14\x1e\xfc\xef\xd9&\x10\x11*x\x88\x81\x9aN\xf7\xddw\xdf{\xf7Y\x0efR\xb4\x11\xb1U\x82\xc5\xfd\x9d!c\x06\x9c'd\xa0\x99X\x82\x92i\xablUZ-*\x08\x84\x82\x02KkQ\xf8\x13\xe4\xaa\x8dn\x94\x906)\xd5B\x02\xeb\xbe\x9c\x01\x85\xc9\x996\x1d\xf8x\x97{\xefy\xe7;\xe7|\xe7\x06\x02\x81\xb1\xe0\xda\x13\xefN\xd1\xca\x8f\xcb-\xbde\xfc\xeepU\xecJ\xc3\xcd@\x86\x96\xc6\xb7^`\xe9"[H\xe4\x18T\x1a\x16p]\xc3\x96\x14\x13\xcbt\xa1tM\x0c\x1c\x0b2,M\xcd\x13qO\x03:\xd089"c1\xcd\x87FI\\\xa8\xbf|\xbc\xbf\x11\\p{_\xe5\xb6\xddn^j\xdd\xec\x0f=z\xb7\xb6\x94)\xebT\xbe\x89\xa3 (esc)
  \x1f6!6p\x00\xc4H`L\x18\x83\xdc\xa6\x8c\x0b\x84\x01\x06\x06s\xb84\x1cn2F4u\x19*\xd4*\x14\x04#a\x8f\x84\xe3\xfe^\xc8OS\xa1\xfc8\xe7\x82\xebj[7\x82@\x97\xb1v\x9dEH4,\xe2\xc2\xd3\xa1\x90\x800\x07\xb9\xc4@\xea\xee\xe4\xc1\xd2\xcf\xe7\xb3\xba[\xf2\xf6X\xdd]C\x1d\x05\xf3\x87\x1f,l\xeeBt\x87\xa5\xf2\xdd\x9e\x90*\xa9kC\xac"!\x17\x12)!c\x000\xd7\x05&\xb5\xa9\xc5\xa8-Ln (esc)
  \x0c|\xf2A\x85\x1a\x85bUy\x9d\xb6\x93(\x8b\xd4\xc4=B/\x8a?\rP'G\x15\x98B\xde\xd6\xa9Zy/\xfb'j+f\xc2\xe3\xb9\xb4\xf5\xea\x98\xf6\xa6sz\xf9{\xc3.\xa4vX*\xdf\x04\x0f\xff[\xb4\x8dGG4\xc1$\xe1:\xb9\xbaq\xf2\xeb\xa9\xfd\xebM\xa3\xc5?\x07\xce\xdc\xda\xc0\xf9\xcd\xef\xbf\xa5\xd3g\xd2\xd2\xa8\xa5uKu\x01(8$\xa6k@\x02(D\x16\x80\x00\x99\x82\x08\xa5\r\x81(t\\f`\xea\x02\xce\xb5\x7f\xba\xac\x02\x8c\\x\x98\x9f\xd5\xb7:0W\xdd6\xbf\xd2\xd3s\xa0k\xbd\xeb\xd8L\xa6	\xa5Q\x86\x91Pc\x80\x98\x8cB,L\x07#\x80\x04\x82\xb6\x8d)\xa3\x08X\x02\x00\xear\x0c-`b\x9b\x18>\xa1\x1b\xf9g\xe9@\xd1\xe9\xca_US{G\xb3\x9f?\x9b\x8d\xd6\x86zR\x91LE\xe8/\xdd& (esc)
  C
  \xd5~u\xb0e#\x08\r\x8c\xd5\xf83\x93\x01B\x95\xe8\x1c\x03\xdb\x92s*\x99`\xcc0\x88\xb4d\xb2\xbd\x85\xc9,\x14\xb7\xf1\xd9\xf2\xe5Ku\x8d\xf5rp\xb6\xee\\\xe0\xc5\xa7C\xd9\xd7\xefe\xda\xe94\xc5\xaa\xde>\x8a\x02I\xcb!\x16\xc1\x10"\x1b\x11\xe0\x02\xc8l\xe9H\x84\xb0\xf4\xa78\xc9-\xf1(\xa9\x15\x0f.\x8c\x8fT\x16\x965\xe9'\xbe\xac6\xaeLtN\x0f\x0e/fJ-\x8d\x08s\x12#\xe7[\xfe\xff\x0b\x17\xb9\xc6KK\xfa\xa2o\xa7\x1e\x87\xfaKb\x8b\xaf?\xcc\xed{z>\xd3\xb8\xbb\xcc}\x8eB\x01\x89\xc6\xbc\x88hO\xa6\x15\xf8\rr4\xb3\xe5 (no-eol) (esc)

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
  var data = [["548001d11f45", [0, 1], [[0, 0, 1, null]], "\u80fd", "test", "1970-01-01", ["unstable", true], ["tip"], ["something"]], ["ba87b23d29ca", [0, 1], [[0, 0, 1, {"color": "FF0000", "width": "3"}]], "branch", "test", "1970-01-01", ["unstable", false], [], []], ["1d22e65f027e", [0, 1], [[0, 0, 1, {"width": "3"}]], "branch", "test", "1970-01-01", ["stable", true], [], []], ["a4f92ed23982", [0, 1], [[0, 0, 1, {"width": "3"}]], "Added tag 1.0 for changeset 2ef0ac749a14", "test", "1970-01-01", ["default", true], [], []], ["2ef0ac749a14", [0, 1], [], "base", "test", "1970-01-01", ["default", false], ["1.0"], ["anotherthing"]]];

ERRORS ENCOUNTERED

  $ cat errors.log
