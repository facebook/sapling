
  $ "$TESTDIR/hghave" pygments || exit 80
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

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/file/tip/primes.py') \
  >     | sed "s/class=\"k\"/class=\"kn\"/g" | sed "s/class=\"mf\"/class=\"mi\"/g"
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  
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
  <li><a href="/annotate/853dcd4de2a6/primes.py">annotate</a></li>
  <li><a href="/log/853dcd4de2a6/primes.py">file log</a></li>
  <li><a href="/raw-file/853dcd4de2a6/primes.py">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>view primes.py @ 0:853dcd4de2a6</h3>
  
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
   <td class="date">Thu Jan 01 00:00:00 1970 +0000 (1970-01-01)</td>
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
  <div class="sourcefirst"> line source</div>
  
  <div class="parity0 source"><a href="#l1" id="l1">     1</a> <span class="c">#!/usr/bin/env python</span></div>
  <div class="parity1 source"><a href="#l2" id="l2">     2</a> </div>
  <div class="parity0 source"><a href="#l3" id="l3">     3</a> <span class="sd">&quot;&quot;&quot;Fun with generators. Corresponding Haskell implementation:</span></div>
  <div class="parity1 source"><a href="#l4" id="l4">     4</a> </div>
  <div class="parity0 source"><a href="#l5" id="l5">     5</a> <span class="sd">primes = 2 : sieve [3, 5..]</span></div>
  <div class="parity1 source"><a href="#l6" id="l6">     6</a> <span class="sd">    where sieve (p:ns) = p : sieve [n | n &lt;- ns, mod n p /= 0]</span></div>
  <div class="parity0 source"><a href="#l7" id="l7">     7</a> <span class="sd">&quot;&quot;&quot;</span></div>
  <div class="parity1 source"><a href="#l8" id="l8">     8</a> </div>
  <div class="parity0 source"><a href="#l9" id="l9">     9</a> <span class="kn">from</span> <span class="nn">itertools</span> <span class="kn">import</span> <span class="n">dropwhile</span><span class="p">,</span> <span class="n">ifilter</span><span class="p">,</span> <span class="n">islice</span><span class="p">,</span> <span class="n">count</span><span class="p">,</span> <span class="n">chain</span></div>
  <div class="parity1 source"><a href="#l10" id="l10">    10</a> </div>
  <div class="parity0 source"><a href="#l11" id="l11">    11</a> <span class="kn">def</span> <span class="nf">primes</span><span class="p">():</span></div>
  <div class="parity1 source"><a href="#l12" id="l12">    12</a>     <span class="sd">&quot;&quot;&quot;Generate all primes.&quot;&quot;&quot;</span></div>
  <div class="parity0 source"><a href="#l13" id="l13">    13</a>     <span class="kn">def</span> <span class="nf">sieve</span><span class="p">(</span><span class="n">ns</span><span class="p">):</span></div>
  <div class="parity1 source"><a href="#l14" id="l14">    14</a>         <span class="n">p</span> <span class="o">=</span> <span class="n">ns</span><span class="o">.</span><span class="n">next</span><span class="p">()</span></div>
  <div class="parity0 source"><a href="#l15" id="l15">    15</a>         <span class="c"># It is important to yield *here* in order to stop the</span></div>
  <div class="parity1 source"><a href="#l16" id="l16">    16</a>         <span class="c"># infinite recursion.</span></div>
  <div class="parity0 source"><a href="#l17" id="l17">    17</a>         <span class="kn">yield</span> <span class="n">p</span></div>
  <div class="parity1 source"><a href="#l18" id="l18">    18</a>         <span class="n">ns</span> <span class="o">=</span> <span class="n">ifilter</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">n</span><span class="p">:</span> <span class="n">n</span> <span class="o">%</span> <span class="n">p</span> <span class="o">!=</span> <span class="mi">0</span><span class="p">,</span> <span class="n">ns</span><span class="p">)</span></div>
  <div class="parity0 source"><a href="#l19" id="l19">    19</a>         <span class="kn">for</span> <span class="n">n</span> <span class="ow">in</span> <span class="n">sieve</span><span class="p">(</span><span class="n">ns</span><span class="p">):</span></div>
  <div class="parity1 source"><a href="#l20" id="l20">    20</a>             <span class="kn">yield</span> <span class="n">n</span></div>
  <div class="parity0 source"><a href="#l21" id="l21">    21</a> </div>
  <div class="parity1 source"><a href="#l22" id="l22">    22</a>     <span class="n">odds</span> <span class="o">=</span> <span class="n">ifilter</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">i</span><span class="p">:</span> <span class="n">i</span> <span class="o">%</span> <span class="mi">2</span> <span class="o">==</span> <span class="mi">1</span><span class="p">,</span> <span class="n">count</span><span class="p">())</span></div>
  <div class="parity0 source"><a href="#l23" id="l23">    23</a>     <span class="kn">return</span> <span class="n">chain</span><span class="p">([</span><span class="mi">2</span><span class="p">],</span> <span class="n">sieve</span><span class="p">(</span><span class="n">dropwhile</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">n</span><span class="p">:</span> <span class="n">n</span> <span class="o">&lt;</span> <span class="mi">3</span><span class="p">,</span> <span class="n">odds</span><span class="p">)))</span></div>
  <div class="parity1 source"><a href="#l24" id="l24">    24</a> </div>
  <div class="parity0 source"><a href="#l25" id="l25">    25</a> <span class="kn">if</span> <span class="n">__name__</span> <span class="o">==</span> <span class="s">&quot;__main__&quot;</span><span class="p">:</span></div>
  <div class="parity1 source"><a href="#l26" id="l26">    26</a>     <span class="kn">import</span> <span class="nn">sys</span></div>
  <div class="parity0 source"><a href="#l27" id="l27">    27</a>     <span class="kn">try</span><span class="p">:</span></div>
  <div class="parity1 source"><a href="#l28" id="l28">    28</a>         <span class="n">n</span> <span class="o">=</span> <span class="nb">int</span><span class="p">(</span><span class="n">sys</span><span class="o">.</span><span class="n">argv</span><span class="p">[</span><span class="mi">1</span><span class="p">])</span></div>
  <div class="parity0 source"><a href="#l29" id="l29">    29</a>     <span class="kn">except</span> <span class="p">(</span><span class="ne">ValueError</span><span class="p">,</span> <span class="ne">IndexError</span><span class="p">):</span></div>
  <div class="parity1 source"><a href="#l30" id="l30">    30</a>         <span class="n">n</span> <span class="o">=</span> <span class="mi">10</span></div>
  <div class="parity0 source"><a href="#l31" id="l31">    31</a>     <span class="n">p</span> <span class="o">=</span> <span class="n">primes</span><span class="p">()</span></div>
  <div class="parity1 source"><a href="#l32" id="l32">    32</a>     <span class="kn">print</span> <span class="s">&quot;The first </span><span class="si">%d</span><span class="s"> primes: </span><span class="si">%s</span><span class="s">&quot;</span> <span class="o">%</span> <span class="p">(</span><span class="n">n</span><span class="p">,</span> <span class="nb">list</span><span class="p">(</span><span class="n">islice</span><span class="p">(</span><span class="n">p</span><span class="p">,</span> <span class="n">n</span><span class="p">)))</span></div>
  <div class="sourcelast"></div>
  </div>
  </div>
  </div>
  
  
  
  </body>
  </html>
  

hgweb fileannotate, html

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/annotate/tip/primes.py') \
  >     | sed "s/class=\"k\"/class=\"kn\"/g" | sed "s/class=\"mi\"/class=\"mf\"/g"
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  
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
  <li class="active">annotate</li>
  <li><a href="/log/853dcd4de2a6/primes.py">file log</a></li>
  <li><a href="/raw-annotate/853dcd4de2a6/primes.py">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2><a href="/">test</a></h2>
  <h3>annotate primes.py @ 0:853dcd4de2a6</h3>
  
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
   <td class="date">Thu Jan 01 00:00:00 1970 +0000 (1970-01-01)</td>
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
  <tr>
   <th class="annotate">rev</th>
   <th class="line">&nbsp;&nbsp;line source</th>
  </tr>
  
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#1"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l1" id="l1">     1</a> <span class="c">#!/usr/bin/env python</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#2"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l2" id="l2">     2</a> </td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#3"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l3" id="l3">     3</a> <span class="sd">&quot;&quot;&quot;Fun with generators. Corresponding Haskell implementation:</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#4"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l4" id="l4">     4</a> </td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#5"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l5" id="l5">     5</a> <span class="sd">primes = 2 : sieve [3, 5..]</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#6"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l6" id="l6">     6</a> <span class="sd">    where sieve (p:ns) = p : sieve [n | n &lt;- ns, mod n p /= 0]</span></td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#7"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l7" id="l7">     7</a> <span class="sd">&quot;&quot;&quot;</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#8"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l8" id="l8">     8</a> </td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#9"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l9" id="l9">     9</a> <span class="kn">from</span> <span class="nn">itertools</span> <span class="kn">import</span> <span class="n">dropwhile</span><span class="p">,</span> <span class="n">ifilter</span><span class="p">,</span> <span class="n">islice</span><span class="p">,</span> <span class="n">count</span><span class="p">,</span> <span class="n">chain</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#10"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l10" id="l10">    10</a> </td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#11"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l11" id="l11">    11</a> <span class="kn">def</span> <span class="nf">primes</span><span class="p">():</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#12"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l12" id="l12">    12</a>     <span class="sd">&quot;&quot;&quot;Generate all primes.&quot;&quot;&quot;</span></td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#13"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l13" id="l13">    13</a>     <span class="kn">def</span> <span class="nf">sieve</span><span class="p">(</span><span class="n">ns</span><span class="p">):</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#14"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l14" id="l14">    14</a>         <span class="n">p</span> <span class="o">=</span> <span class="n">ns</span><span class="o">.</span><span class="n">next</span><span class="p">()</span></td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#15"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l15" id="l15">    15</a>         <span class="c"># It is important to yield *here* in order to stop the</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#16"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l16" id="l16">    16</a>         <span class="c"># infinite recursion.</span></td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#17"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l17" id="l17">    17</a>         <span class="kn">yield</span> <span class="n">p</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#18"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l18" id="l18">    18</a>         <span class="n">ns</span> <span class="o">=</span> <span class="n">ifilter</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">n</span><span class="p">:</span> <span class="n">n</span> <span class="o">%</span> <span class="n">p</span> <span class="o">!=</span> <span class="mf">0</span><span class="p">,</span> <span class="n">ns</span><span class="p">)</span></td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#19"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l19" id="l19">    19</a>         <span class="kn">for</span> <span class="n">n</span> <span class="ow">in</span> <span class="n">sieve</span><span class="p">(</span><span class="n">ns</span><span class="p">):</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#20"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l20" id="l20">    20</a>             <span class="kn">yield</span> <span class="n">n</span></td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#21"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l21" id="l21">    21</a> </td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#22"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l22" id="l22">    22</a>     <span class="n">odds</span> <span class="o">=</span> <span class="n">ifilter</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">i</span><span class="p">:</span> <span class="n">i</span> <span class="o">%</span> <span class="mf">2</span> <span class="o">==</span> <span class="mf">1</span><span class="p">,</span> <span class="n">count</span><span class="p">())</span></td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#23"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l23" id="l23">    23</a>     <span class="kn">return</span> <span class="n">chain</span><span class="p">([</span><span class="mf">2</span><span class="p">],</span> <span class="n">sieve</span><span class="p">(</span><span class="n">dropwhile</span><span class="p">(</span><span class="kn">lambda</span> <span class="n">n</span><span class="p">:</span> <span class="n">n</span> <span class="o">&lt;</span> <span class="mf">3</span><span class="p">,</span> <span class="n">odds</span><span class="p">)))</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#24"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l24" id="l24">    24</a> </td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#25"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l25" id="l25">    25</a> <span class="kn">if</span> <span class="n">__name__</span> <span class="o">==</span> <span class="s">&quot;__main__&quot;</span><span class="p">:</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#26"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l26" id="l26">    26</a>     <span class="kn">import</span> <span class="nn">sys</span></td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#27"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l27" id="l27">    27</a>     <span class="kn">try</span><span class="p">:</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#28"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l28" id="l28">    28</a>         <span class="n">n</span> <span class="o">=</span> <span class="nb">int</span><span class="p">(</span><span class="n">sys</span><span class="o">.</span><span class="n">argv</span><span class="p">[</span><span class="mf">1</span><span class="p">])</span></td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#29"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l29" id="l29">    29</a>     <span class="kn">except</span> <span class="p">(</span><span class="ne">ValueError</span><span class="p">,</span> <span class="ne">IndexError</span><span class="p">):</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#30"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l30" id="l30">    30</a>         <span class="n">n</span> <span class="o">=</span> <span class="mf">10</span></td>
  </tr>
  <tr class="parity0">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#31"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l31" id="l31">    31</a>     <span class="n">p</span> <span class="o">=</span> <span class="n">primes</span><span class="p">()</span></td>
  </tr>
  <tr class="parity1">
  <td class="annotate">
  <a href="/annotate/853dcd4de2a6/primes.py#32"
  title="853dcd4de2a6: a">test@0</a>
  </td>
  <td class="source"><a href="#l32" id="l32">    32</a>     <span class="kn">print</span> <span class="s">&quot;The first </span><span class="si">%d</span><span class="s"> primes: </span><span class="si">%s</span><span class="s">&quot;</span> <span class="o">%</span> <span class="p">(</span><span class="n">n</span><span class="p">,</span> <span class="nb">list</span><span class="p">(</span><span class="n">islice</span><span class="p">(</span><span class="n">p</span><span class="p">,</span> <span class="n">n</span><span class="p">)))</span></td>
  </tr>
  </table>
  </div>
  </div>
  </div>
  
  
  
  </body>
  </html>
  

hgweb fileannotate, raw

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/annotate/tip/primes.py?style=raw') \
  >     | sed "s/test@//" > a
  $ echo "200 Script output follows" > b
  $ echo "" >> b
  $ echo "" >> b
  $ hg annotate "primes.py" >> b
  $ echo "" >> b
  $ echo "" >> b
  $ echo "" >> b
  $ echo "" >> b
  $ diff -u b a
  $ echo
  

hgweb filerevision, raw

  $ ("$TESTDIR/get-with-headers.py" localhost:$HGPORT '/file/tip/primes.py?style=raw') \
  >     > a
  $ echo "200 Script output follows" > b
  $ echo "" >> b
  $ hg cat primes.py >> b
  $ diff -u b a
  $ echo
  

hgweb highlightcss friendly

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/highlightcss' > out
  $ head -n 4 out
  200 Script output follows
  
  /* pygments_style = friendly */
  
  $ rm out

errors encountered

  $ cat errors.log
  $ "$TESTDIR/killdaemons.py"

Change the pygments style

  $ cat > .hg/hgrc <<EOF
  > [web]
  > pygments_style = fruity
  > EOF

hg serve again

  $ hg serve -p $HGPORT -d -n test --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

hgweb highlightcss fruity

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/highlightcss' > out
  $ head -n 4 out
  200 Script output follows
  
  /* pygments_style = fruity */
  
  $ rm out

errors encountered

  $ cat errors.log
  $ cd ..
  $ hg init eucjp
  $ cd eucjp
  $ python -c 'print("\265\376")' >> eucjp.txt  # Japanese kanji "Kyo"
  $ hg ci -Ama
  adding eucjp.txt
  $ hgserveget () {
  >     "$TESTDIR/killdaemons.py"
  >     echo % HGENCODING="$1" hg serve
  >     HGENCODING="$1" hg serve -p $HGPORT -d -n test --pid-file=hg.pid -E errors.log
  >     cat hg.pid >> $DAEMON_PIDS
  > 
  >     echo % hgweb filerevision, html
  >     "$TESTDIR/get-with-headers.py" localhost:$HGPORT "/file/tip/$2" \
  >         | grep '<div class="parity0 source">'
  >     echo % errors encountered
  >     cat errors.log
  > }
  $ hgserveget euc-jp eucjp.txt
  % HGENCODING=euc-jp hg serve
  % hgweb filerevision, html
  <div class="parity0 source"><a href="#l1" id="l1">     1</a> \xb5\xfe</div> (esc)
  % errors encountered
  $ hgserveget utf-8 eucjp.txt
  % HGENCODING=utf-8 hg serve
  % hgweb filerevision, html
  <div class="parity0 source"><a href="#l1" id="l1">     1</a> \xef\xbf\xbd\xef\xbf\xbd</div> (esc)
  % errors encountered
  $ hgserveget us-ascii eucjp.txt
  % HGENCODING=us-ascii hg serve
  % hgweb filerevision, html
  <div class="parity0 source"><a href="#l1" id="l1">     1</a> ??</div>
  % errors encountered
