#require pygments serve

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > highlight =
  > [web]
  > pygments_style = friendly
  > EOF
  $ hg init test
  $ cd test

create random Python file to exercise Pygments

  $ cat <<EOF > primes.py
  > #!/usr/bin/env python
  > 
  > """Fun with generators. Corresponding Haskell implementation:
  > 
  > primes = 2 : sieve [3, 5..]
  >     where sieve (p:ns) = p : sieve [n | n <- ns, mod n p /= 0]
  > """
  > 
  > from itertools import dropwhile, ifilter, islice, count, chain
  > 
  > def primes():
  >     """Generate all primes."""
  >     def sieve(ns):
  >         p = ns.next()
  >         # It is important to yield *here* in order to stop the
  >         # infinite recursion.
  >         yield p
  >         ns = ifilter(lambda n: n % p != 0, ns)
  >         for n in sieve(ns):
  >             yield n
  > 
  >     odds = ifilter(lambda i: i % 2 == 1, count())
  >     return chain([2], sieve(dropwhile(lambda n: n < 3, odds)))
  > 
  > if __name__ == "__main__":
  >     import sys
  >     try:
  >         n = int(sys.argv[1])
  >     except (ValueError, IndexError):
  >         n = 10
  >     p = primes()
  >     print "The first %d primes: %s" % (n, list(islice(p, n)))
  > EOF
  $ hg ci -Ama
  adding primes.py

hg serve

  $ hg serve -p $HGPORT -d -n test --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

hgweb filerevision, html

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT 'file/tip/primes.py') \
  >     | sed "s/class=\"k\"/class=\"kn\"/g" | sed "s/class=\"mf\"/class=\"mi\"/g"
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <link rel="stylesheet" href="/highlightcss" type="text/css" />
  <title>test: 853dcd4de2a6 primes.py</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/853dcd4de2a6">log</a></li>
  <li><a href="/graph/853dcd4de2a6">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/853dcd4de2a6">changeset</a></li>
  <li><a href="/file/853dcd4de2a6/">browse</a></li>
  </ul>
  <ul>
  <li class="active">file</li>
  <li><a href="/file/tip/primes.py">latest</a></li>
  <li><a href="/diff/853dcd4de2a6/primes.py">diff</a></li>
  <li><a href="/comparison/853dcd4de2a6/primes.py">comparison</a></li>
  <li><a href="/annotate/853dcd4de2a6/primes.py">annotate</a></li>
  <li><a href="/log/853dcd4de2a6/primes.py">file log</a></li>
  <li><a href="/raw-file/853dcd4de2a6/primes.py">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>view primes.py @ 0:853dcd4de2a6 <span class="tag">tip</span> </h3>
  
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
   <td class="author"></td>
  </tr>
  </table>
  
  <div class="overflow">
  <div class="sourcefirst linewraptoggle">line wrap: <a class="linewraplink" href="javascript:toggleLinewrap()">on</a></div>
  <div class="sourcefirst"> line source</div>
  <pre class="sourcelines stripes4 wrap">
  <span id="l1"><span class="c">#!/usr/bin/env python</span></span><a href="#l1"></a>
  <span id="l2"></span><a href="#l2"></a>
  <span id="l3"><span class="sd">&quot;&quot;&quot;Fun with generators. Corresponding Haskell implementation:</span></span><a href="#l3"></a>
  <span id="l4"></span><a href="#l4"></a>
  <span id="l5"><span class="sd">primes = 2 : sieve [3, 5..]</span></span><a href="#l5"></a>
  <span id="l6"><span class="sd">    where sieve (p:ns) = p : sieve [n | n &lt;- ns, mod n p /= 0]</span></span><a href="#l6"></a>
  <span id="l7"><span class="sd">&quot;&quot;&quot;</span></span><a href="#l7"></a>
  <span id="l8"></span><a href="#l8"></a>
  <span id="l9"><span class="kn">from</span> <span class="nn">itertools</span> <span class="kn">import</span> <span class="n">dropwhile</span><span class="p">,</span> <span class="n">ifilter</span><span class="p">,</span> <span class="n">islice</span><span class="p">,</span> <span class="n">count</span><span class="p">,</span> <span class="n">chain</span></span><a href="#l9"></a>
  <span id="l10"></span><a href="#l10"></a>
  <span id="l11"><span class="kn">def</span> <span class="nf">primes</span><span class="p">():</span></span><a href="#l11"></a>
  <span id="l12">    <span class="sd">&quot;&quot;&quot;Generate all primes.&quot;&quot;&quot;</span></span><a href="#l12"></a>
  <span id="l13">    <span class="kn">def</span> <span class="nf">sieve</span><span class="p">(</span><span class="n">ns</span><span class="p">):</span></span><a href="#l13"></a>
  <span id="l14">        <span class="n">p</span> <span class="o">=</span> <span class="n">ns</span><span class="o">.</span><span class="n">next</span><span class="p">()</span></span><a href="#l14"></a>
  <span id="l15">        <span class="c"># It is important to yield *here* in order to stop the</span></span><a href="#l15"></a>
  <span id="l16">        <span class="c"># infinite recursion.</span></span><a href="#l16"></a>
  <span id="l17">        <span class="kn">yield</span> <span class="n">p</span></span><a href="#l17"></a>
  <span id="l18">        <span class="n">ns</span> <span class="o">=</span> <span class="n">ifilter</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">n</span><span class="p">:</span> <span class="n">n</span> <span class="o">%</span> <span class="n">p</span> <span class="o">!=</span> <span class="mi">0</span><span class="p">,</span> <span class="n">ns</span><span class="p">)</span></span><a href="#l18"></a>
  <span id="l19">        <span class="kn">for</span> <span class="n">n</span> <span class="ow">in</span> <span class="n">sieve</span><span class="p">(</span><span class="n">ns</span><span class="p">):</span></span><a href="#l19"></a>
  <span id="l20">            <span class="kn">yield</span> <span class="n">n</span></span><a href="#l20"></a>
  <span id="l21"></span><a href="#l21"></a>
  <span id="l22">    <span class="n">odds</span> <span class="o">=</span> <span class="n">ifilter</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">i</span><span class="p">:</span> <span class="n">i</span> <span class="o">%</span> <span class="mi">2</span> <span class="o">==</span> <span class="mi">1</span><span class="p">,</span> <span class="n">count</span><span class="p">())</span></span><a href="#l22"></a>
  <span id="l23">    <span class="kn">return</span> <span class="n">chain</span><span class="p">([</span><span class="mi">2</span><span class="p">],</span> <span class="n">sieve</span><span class="p">(</span><span class="n">dropwhile</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">n</span><span class="p">:</span> <span class="n">n</span> <span class="o">&lt;</span> <span class="mi">3</span><span class="p">,</span> <span class="n">odds</span><span class="p">)))</span></span><a href="#l23"></a>
  <span id="l24"></span><a href="#l24"></a>
  <span id="l25"><span class="kn">if</span> <span class="n">__name__</span> <span class="o">==</span> <span class="s">&quot;__main__&quot;</span><span class="p">:</span></span><a href="#l25"></a>
  <span id="l26">    <span class="kn">import</span> <span class="nn">sys</span></span><a href="#l26"></a>
  <span id="l27">    <span class="kn">try</span><span class="p">:</span></span><a href="#l27"></a>
  <span id="l28">        <span class="n">n</span> <span class="o">=</span> <span class="nb">int</span><span class="p">(</span><span class="n">sys</span><span class="o">.</span><span class="n">argv</span><span class="p">[</span><span class="mi">1</span><span class="p">])</span></span><a href="#l28"></a>
  <span id="l29">    <span class="kn">except</span> <span class="p">(</span><span class="ne">ValueError</span><span class="p">,</span> <span class="ne">IndexError</span><span class="p">):</span></span><a href="#l29"></a>
  <span id="l30">        <span class="n">n</span> <span class="o">=</span> <span class="mi">10</span></span><a href="#l30"></a>
  <span id="l31">    <span class="n">p</span> <span class="o">=</span> <span class="n">primes</span><span class="p">()</span></span><a href="#l31"></a>
  <span id="l32">    <span class="kn">print</span> <span class="s">&quot;The first </span><span class="si">%d</span><span class="s"> primes: </span><span class="si">%s</span><span class="s">&quot;</span> <span class="o">%</span> <span class="p">(</span><span class="n">n</span><span class="p">,</span> <span class="nb">list</span><span class="p">(</span><span class="n">islice</span><span class="p">(</span><span class="n">p</span><span class="p">,</span> <span class="n">n</span><span class="p">)))</span></span><a href="#l32"></a></pre>
  <div class="sourcelast"></div>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

hgweb fileannotate, html

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT 'annotate/tip/primes.py') \
  >     | sed "s/class=\"k\"/class=\"kn\"/g" | sed "s/class=\"mi\"/class=\"mf\"/g"
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <link rel="stylesheet" href="/highlightcss" type="text/css" />
  <title>test: primes.py annotate</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/853dcd4de2a6">log</a></li>
  <li><a href="/graph/853dcd4de2a6">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  
  <ul>
  <li><a href="/rev/853dcd4de2a6">changeset</a></li>
  <li><a href="/file/853dcd4de2a6/">browse</a></li>
  </ul>
  <ul>
  <li><a href="/file/853dcd4de2a6/primes.py">file</a></li>
  <li><a href="/file/tip/primes.py">latest</a></li>
  <li><a href="/diff/853dcd4de2a6/primes.py">diff</a></li>
  <li><a href="/comparison/853dcd4de2a6/primes.py">comparison</a></li>
  <li class="active">annotate</li>
  <li><a href="/log/853dcd4de2a6/primes.py">file log</a></li>
  <li><a href="/raw-annotate/853dcd4de2a6/primes.py">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>annotate primes.py @ 0:853dcd4de2a6</h3>
  
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
   <td class="author"></td>
  </tr>
  </table>
  
  <div class="overflow">
  <table class="bigtable">
  <thead>
  <tr>
   <th class="annotate">rev</th>
   <th class="line">&nbsp;&nbsp;line source</th>
  </tr>
  </thead>
  <tbody class="stripes2">
    
  <tr id="l1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l1"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l1">     1</a> <span class="c">#!/usr/bin/env python</span></td>
  </tr>
  <tr id="l2">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l2"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l2">     2</a> </td>
  </tr>
  <tr id="l3">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l3"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l3">     3</a> <span class="sd">&quot;&quot;&quot;Fun with generators. Corresponding Haskell implementation:</span></td>
  </tr>
  <tr id="l4">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l4"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l4">     4</a> </td>
  </tr>
  <tr id="l5">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l5"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l5">     5</a> <span class="sd">primes = 2 : sieve [3, 5..]</span></td>
  </tr>
  <tr id="l6">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l6"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l6">     6</a> <span class="sd">    where sieve (p:ns) = p : sieve [n | n &lt;- ns, mod n p /= 0]</span></td>
  </tr>
  <tr id="l7">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l7"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l7">     7</a> <span class="sd">&quot;&quot;&quot;</span></td>
  </tr>
  <tr id="l8">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l8"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l8">     8</a> </td>
  </tr>
  <tr id="l9">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l9"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l9">     9</a> <span class="kn">from</span> <span class="nn">itertools</span> <span class="kn">import</span> <span class="n">dropwhile</span><span class="p">,</span> <span class="n">ifilter</span><span class="p">,</span> <span class="n">islice</span><span class="p">,</span> <span class="n">count</span><span class="p">,</span> <span class="n">chain</span></td>
  </tr>
  <tr id="l10">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l10"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l10">    10</a> </td>
  </tr>
  <tr id="l11">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l11"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l11">    11</a> <span class="kn">def</span> <span class="nf">primes</span><span class="p">():</span></td>
  </tr>
  <tr id="l12">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l12"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l12">    12</a>     <span class="sd">&quot;&quot;&quot;Generate all primes.&quot;&quot;&quot;</span></td>
  </tr>
  <tr id="l13">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l13"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l13">    13</a>     <span class="kn">def</span> <span class="nf">sieve</span><span class="p">(</span><span class="n">ns</span><span class="p">):</span></td>
  </tr>
  <tr id="l14">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l14"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l14">    14</a>         <span class="n">p</span> <span class="o">=</span> <span class="n">ns</span><span class="o">.</span><span class="n">next</span><span class="p">()</span></td>
  </tr>
  <tr id="l15">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l15"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l15">    15</a>         <span class="c"># It is important to yield *here* in order to stop the</span></td>
  </tr>
  <tr id="l16">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l16"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l16">    16</a>         <span class="c"># infinite recursion.</span></td>
  </tr>
  <tr id="l17">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l17"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l17">    17</a>         <span class="kn">yield</span> <span class="n">p</span></td>
  </tr>
  <tr id="l18">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l18"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l18">    18</a>         <span class="n">ns</span> <span class="o">=</span> <span class="n">ifilter</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">n</span><span class="p">:</span> <span class="n">n</span> <span class="o">%</span> <span class="n">p</span> <span class="o">!=</span> <span class="mf">0</span><span class="p">,</span> <span class="n">ns</span><span class="p">)</span></td>
  </tr>
  <tr id="l19">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l19"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l19">    19</a>         <span class="kn">for</span> <span class="n">n</span> <span class="ow">in</span> <span class="n">sieve</span><span class="p">(</span><span class="n">ns</span><span class="p">):</span></td>
  </tr>
  <tr id="l20">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l20"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l20">    20</a>             <span class="kn">yield</span> <span class="n">n</span></td>
  </tr>
  <tr id="l21">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l21"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l21">    21</a> </td>
  </tr>
  <tr id="l22">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l22"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l22">    22</a>     <span class="n">odds</span> <span class="o">=</span> <span class="n">ifilter</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">i</span><span class="p">:</span> <span class="n">i</span> <span class="o">%</span> <span class="mf">2</span> <span class="o">==</span> <span class="mf">1</span><span class="p">,</span> <span class="n">count</span><span class="p">())</span></td>
  </tr>
  <tr id="l23">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l23"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l23">    23</a>     <span class="kn">return</span> <span class="n">chain</span><span class="p">([</span><span class="mf">2</span><span class="p">],</span> <span class="n">sieve</span><span class="p">(</span><span class="n">dropwhile</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">n</span><span class="p">:</span> <span class="n">n</span> <span class="o">&lt;</span> <span class="mf">3</span><span class="p">,</span> <span class="n">odds</span><span class="p">)))</span></td>
  </tr>
  <tr id="l24">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l24"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l24">    24</a> </td>
  </tr>
  <tr id="l25">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l25"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l25">    25</a> <span class="kn">if</span> <span class="n">__name__</span> <span class="o">==</span> <span class="s">&quot;__main__&quot;</span><span class="p">:</span></td>
  </tr>
  <tr id="l26">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l26"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l26">    26</a>     <span class="kn">import</span> <span class="nn">sys</span></td>
  </tr>
  <tr id="l27">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l27"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l27">    27</a>     <span class="kn">try</span><span class="p">:</span></td>
  </tr>
  <tr id="l28">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l28"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l28">    28</a>         <span class="n">n</span> <span class="o">=</span> <span class="nb">int</span><span class="p">(</span><span class="n">sys</span><span class="o">.</span><span class="n">argv</span><span class="p">[</span><span class="mf">1</span><span class="p">])</span></td>
  </tr>
  <tr id="l29">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l29"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l29">    29</a>     <span class="kn">except</span> <span class="p">(</span><span class="ne">ValueError</span><span class="p">,</span> <span class="ne">IndexError</span><span class="p">):</span></td>
  </tr>
  <tr id="l30">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l30"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l30">    30</a>         <span class="n">n</span> <span class="o">=</span> <span class="mf">10</span></td>
  </tr>
  <tr id="l31">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l31"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l31">    31</a>     <span class="n">p</span> <span class="o">=</span> <span class="n">primes</span><span class="p">()</span></td>
  </tr>
  <tr id="l32">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#l32"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l32">    32</a>     <span class="kn">print</span> <span class="s">&quot;The first </span><span class="si">%d</span><span class="s"> primes: </span><span class="si">%s</span><span class="s">&quot;</span> <span class="o">%</span> <span class="p">(</span><span class="n">n</span><span class="p">,</span> <span class="nb">list</span><span class="p">(</span><span class="n">islice</span><span class="p">(</span><span class="n">p</span><span class="p">,</span> <span class="n">n</span><span class="p">)))</span></td>
  </tr>
  </tbody>
  </table>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

hgweb fileannotate, raw

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT 'annotate/tip/primes.py?style=raw') \
  >     | sed "s/test@//" > a
  $ echo "200 Script output follows" > b
  $ echo "" >> b
  $ echo "" >> b
  $ hg annotate "primes.py" >> b
  $ echo "" >> b
  $ echo "" >> b
  $ echo "" >> b
  $ echo "" >> b
  $ cmp b a || diff -u b a

hgweb filerevision, raw

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT 'file/tip/primes.py?style=raw') \
  >     > a
  $ echo "200 Script output follows" > b
  $ echo "" >> b
  $ hg cat primes.py >> b
  $ cmp b a || diff -u b a

hgweb highlightcss friendly

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'highlightcss' > out
  $ head -n 4 out
  200 Script output follows
  
  /* pygments_style = friendly */
  
  $ rm out

errors encountered

  $ cat errors.log
  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS

Change the pygments style

  $ cat > .hg/hgrc <<EOF
  > [web]
  > pygments_style = fruity
  > EOF

hg serve again

  $ hg serve -p $HGPORT -d -n test --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

hgweb highlightcss fruity

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'highlightcss' > out
  $ head -n 4 out
  200 Script output follows
  
  /* pygments_style = fruity */
  
  $ rm out

errors encountered

  $ cat errors.log
  $ cd ..
  $ hg init eucjp
  $ cd eucjp
  $ $PYTHON -c 'print("\265\376")' >> eucjp.txt  # Japanese kanji "Kyo"
  $ hg ci -Ama
  adding eucjp.txt
  $ hgserveget () {
  >     "$TESTDIR/killdaemons.py" $DAEMON_PIDS
  >     echo % HGENCODING="$1" hg serve
  >     HGENCODING="$1" hg serve -p $HGPORT -d -n test --pid-file=hg.pid -E errors.log
  >     cat hg.pid >> $DAEMON_PIDS
  > 
  >     echo % hgweb filerevision, html
  >     "$TESTDIR/get-with-headers.py" localhost:$HGPORT "file/tip/$2" \
  >         | grep '<div class="parity0 source">'
  >     echo % errors encountered
  >     cat errors.log
  > }
  $ hgserveget euc-jp eucjp.txt
  % HGENCODING=euc-jp hg serve
  % hgweb filerevision, html
  % errors encountered
  $ hgserveget utf-8 eucjp.txt
  % HGENCODING=utf-8 hg serve
  % hgweb filerevision, html
  % errors encountered
  $ hgserveget us-ascii eucjp.txt
  % HGENCODING=us-ascii hg serve
  % hgweb filerevision, html
  % errors encountered

  $ cd ..
