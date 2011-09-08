Some tests for hgweb in an empty repository

  $ hg init test
  $ cd test
  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/shortlog')
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
  <li><a href="/graph/000000000000">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/000000000000">changeset</a></li>
  <li><a href="/file/000000000000">browse</a></li>
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
  <a href="/shortlog/-1?revcount=30">less</a>
  <a href="/shortlog/-1?revcount=120">more</a>
  | rev -1: <a href="/shortlog/000000000000">(0)</a> <a href="/shortlog/tip">tip</a> 
  </div>
  
  <table class="bigtable">
   <tr>
    <th class="age">age</th>
    <th class="author">author</th>
    <th class="description">description</th>
   </tr>
  
  </table>
  
  <div class="navigate">
  <a href="/shortlog/-1?revcount=30">less</a>
  <a href="/shortlog/-1?revcount=120">more</a>
  | rev -1: <a href="/shortlog/000000000000">(0)</a> <a href="/shortlog/tip">tip</a> 
  </div>
  
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/log')
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
  <li><a href="/graph/000000000000">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/000000000000">changeset</a></li>
  <li><a href="/file/000000000000">browse</a></li>
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
  <a href="/shortlog/-1?revcount=5">less</a>
  <a href="/shortlog/-1?revcount=20">more</a>
  | rev -1: <a href="/shortlog/000000000000">(0)</a> <a href="/shortlog/tip">tip</a> 
  </div>
  
  <table class="bigtable">
   <tr>
    <th class="age">age</th>
    <th class="author">author</th>
    <th class="description">description</th>
   </tr>
  
  </table>
  
  <div class="navigate">
  <a href="/shortlog/-1?revcount=5">less</a>
  <a href="/shortlog/-1?revcount=20">more</a>
  | rev -1: <a href="/shortlog/000000000000">(0)</a> <a href="/shortlog/tip">tip</a> 
  </div>
  
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/graph')
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: revision graph</title>
  <link rel="alternate" type="application/atom+xml"
     href="/atom-log" title="Atom feed for test: log" />
  <link rel="alternate" type="application/rss+xml"
     href="/rss-log" title="RSS feed for test: log" />
  <!--[if IE]><script type="text/javascript" src="/static/excanvas.js"></script><![endif]-->
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/000000000000">log</a></li>
  <li class="active">graph</li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/000000000000">changeset</a></li>
  <li><a href="/file/000000000000">browse</a></li>
  </ul>
  <ul>
   <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>graph</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">find changesets by author, revision,
  files, or words in the commit message</div>
  </form>
  
  <div class="navigate">
  <a href="/graph/-1?revcount=30">less</a>
  <a href="/graph/-1?revcount=120">more</a>
  | rev -1: <a href="/graph/000000000000">(0)</a> <a href="/graph/tip">tip</a> 
  </div>
  
  <noscript><p>The revision graph only works with JavaScript-enabled browsers.</p></noscript>
  
  <div id="wrapper">
  <ul id="nodebgs"></ul>
  <canvas id="graph" width="480" height="12"></canvas>
  <ul id="graphnodes"></ul>
  </div>
  
  <script type="text/javascript">
  <!-- hide script content
  
  var data = [];
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
  revlink += '<a href="/rev/_NODEID" title="_NODEID">_DESC</a>';
  revlink += '</span>_TAGS<span class="info">_DATE, by _USER</span></li>';
  
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
  			tagspan += '<span class="branchhead" title="' + cur[6][0] + '">';
  			tagspan += cur[6][0] + '</span> ';
  		} else if (!cur[6][1] && cur[6][0] != 'default') {
  			tagspan += '<span class="branchname" title="' + cur[6][0] + '">';
  			tagspan += cur[6][0] + '</span> ';
  		}
  		if (cur[7].length) {
  			for (var t in cur[7]) {
  				var tag = cur[7][t];
  				tagspan += '<span class="tag">' + tag + '</span> ';
  			}
  		}
  		if (cur[8].length) {
  			for (var b in cur[8]) {
  				var bookmark = cur[8][b];
  				tagspan += '<span class="tag">' + bookmark + '</span> ';
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
  
  <div class="navigate">
  <a href="/graph/-1?revcount=30">less</a>
  <a href="/graph/-1?revcount=120">more</a>
  | rev -1: <a href="/graph/000000000000">(0)</a> <a href="/graph/tip">tip</a> 
  </div>
  
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/file')
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: 000000000000 /</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/000000000000">log</a></li>
  <li><a href="/graph/000000000000">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/000000000000">changeset</a></li>
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
  <h3>directory / @ -1:000000000000 <span class="tag">tip</span> </h3>
  
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
    <td class="name"><a href="/file/000000000000/">[up]</a></td>
    <td class="size"></td>
    <td class="permissions">drwxr-xr-x</td>
  </tr>
  
  
  </table>
  </div>
  </div>
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
