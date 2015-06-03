#require serve

Test symbolic revision usage in links produced by hgweb pages. There are
multiple issues related to this:
- issue2296
- issue2826
- issue3594
- issue3634

Set up the repo

  $ hg init test
  $ cd test
  $ echo 0 > foo
  $ mkdir dir
  $ echo 0 > dir/bar
  $ hg ci -Am 'first'
  adding dir/bar
  adding foo
  $ echo 1 >> foo
  $ hg ci -m 'second'
  $ echo 2 >> foo
  $ hg ci -m 'third'
  $ hg bookmark -r1 xyzzy

  $ hg log -G --template '{rev}:{node|short} {tags} {bookmarks}\n'
  @  2:9d8c40cba617 tip
  |
  o  1:a7c1559b7bba  xyzzy
  |
  o  0:43c799df6e75
  
  $ hg serve --config web.allow_archive=zip -n test -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

  $ REVLINKS='href=[^>]+(rev=|/)(43c799df6e75|0|a7c1559b7bba|1|xyzzy|9d8c40cba617|2|tip)'

(De)referencing symbolic revisions (paper)

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog?style=paper' | egrep $REVLINKS
  <li><a href="/graph/9d8c40cba617?style=paper">graph</a></li>
  <li><a href="/rev/9d8c40cba617?style=paper">changeset</a></li>
  <li><a href="/file/9d8c40cba617?style=paper">browse</a></li>
  <a href="/archive/tip.zip">zip</a>
  <a href="/shortlog/2?revcount=30&style=paper">less</a>
  <a href="/shortlog/2?revcount=120&style=paper">more</a>
  | rev 2: <a href="/shortlog/43c799df6e75?style=paper">(0)</a> <a href="/shortlog/tip?style=paper">tip</a> 
     <a href="/rev/9d8c40cba617?style=paper">third</a>
     <a href="/rev/a7c1559b7bba?style=paper">second</a>
     <a href="/rev/43c799df6e75?style=paper">first</a>
  <a href="/shortlog/2?revcount=30&style=paper">less</a>
  <a href="/shortlog/2?revcount=120&style=paper">more</a>
  | rev 2: <a href="/shortlog/43c799df6e75?style=paper">(0)</a> <a href="/shortlog/tip?style=paper">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'graph?style=paper' | egrep $REVLINKS
  <li><a href="/shortlog/9d8c40cba617?style=paper">log</a></li>
  <li><a href="/rev/9d8c40cba617?style=paper">changeset</a></li>
  <li><a href="/file/9d8c40cba617?style=paper">browse</a></li>
  <a href="/graph/2?revcount=30&style=paper">less</a>
  <a href="/graph/2?revcount=120&style=paper">more</a>
  | rev 2: <a href="/graph/43c799df6e75?style=paper">(0)</a> <a href="/graph/tip?style=paper">tip</a> 
  <a href="/graph/2?revcount=30&style=paper">less</a>
  <a href="/graph/2?revcount=120&style=paper">more</a>
  | rev 2: <a href="/graph/43c799df6e75?style=paper">(0)</a> <a href="/graph/tip?style=paper">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file?style=paper' | egrep $REVLINKS
  <li><a href="/shortlog/9d8c40cba617?style=paper">log</a></li>
  <li><a href="/graph/9d8c40cba617?style=paper">graph</a></li>
  <li><a href="/rev/9d8c40cba617?style=paper">changeset</a></li>
  <a href="/archive/9d8c40cba617.zip">zip</a>
    <td class="name"><a href="/file/9d8c40cba617/?style=paper">[up]</a></td>
  <a href="/file/9d8c40cba617/dir?style=paper">
  <a href="/file/9d8c40cba617/dir/?style=paper">
  <a href="/file/9d8c40cba617/foo?style=paper">

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog?style=paper&rev=all()' | egrep $REVLINKS
     <a href="/rev/9d8c40cba617?style=paper">third</a>
     <a href="/rev/a7c1559b7bba?style=paper">second</a>
     <a href="/rev/43c799df6e75?style=paper">first</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'rev/xyzzy?style=paper' | egrep $REVLINKS
   <li><a href="/shortlog/a7c1559b7bba?style=paper">log</a></li>
   <li><a href="/graph/a7c1559b7bba?style=paper">graph</a></li>
   <li><a href="/raw-rev/a7c1559b7bba?style=paper">raw</a></li>
   <li><a href="/file/a7c1559b7bba?style=paper">browse</a></li>
  <a href="/archive/a7c1559b7bba.zip">zip</a>
   <td class="author"><a href="/rev/43c799df6e75?style=paper">43c799df6e75</a> </td>
   <td class="author"> <a href="/rev/9d8c40cba617?style=paper">9d8c40cba617</a></td>
   <td class="files"><a href="/file/a7c1559b7bba/foo?style=paper">foo</a> </td>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog/xyzzy?style=paper' | egrep $REVLINKS
  <li><a href="/graph/a7c1559b7bba?style=paper">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=paper">changeset</a></li>
  <li><a href="/file/a7c1559b7bba?style=paper">browse</a></li>
  <a href="/archive/tip.zip">zip</a>
  <a href="/shortlog/1?revcount=30&style=paper">less</a>
  <a href="/shortlog/1?revcount=120&style=paper">more</a>
  | rev 1: <a href="/shortlog/43c799df6e75?style=paper">(0)</a> <a href="/shortlog/tip?style=paper">tip</a> 
     <a href="/rev/a7c1559b7bba?style=paper">second</a>
     <a href="/rev/43c799df6e75?style=paper">first</a>
  <a href="/shortlog/1?revcount=30&style=paper">less</a>
  <a href="/shortlog/1?revcount=120&style=paper">more</a>
  | rev 1: <a href="/shortlog/43c799df6e75?style=paper">(0)</a> <a href="/shortlog/tip?style=paper">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'graph/xyzzy?style=paper' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=paper">log</a></li>
  <li><a href="/rev/a7c1559b7bba?style=paper">changeset</a></li>
  <li><a href="/file/a7c1559b7bba?style=paper">browse</a></li>
  <a href="/graph/1?revcount=30&style=paper">less</a>
  <a href="/graph/1?revcount=120&style=paper">more</a>
  | rev 1: <a href="/graph/43c799df6e75?style=paper">(0)</a> <a href="/graph/tip?style=paper">tip</a> 
  <a href="/graph/1?revcount=30&style=paper">less</a>
  <a href="/graph/1?revcount=120&style=paper">more</a>
  | rev 1: <a href="/graph/43c799df6e75?style=paper">(0)</a> <a href="/graph/tip?style=paper">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/xyzzy?style=paper' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=paper">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=paper">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=paper">changeset</a></li>
  <a href="/archive/a7c1559b7bba.zip">zip</a>
    <td class="name"><a href="/file/a7c1559b7bba/?style=paper">[up]</a></td>
  <a href="/file/a7c1559b7bba/dir?style=paper">
  <a href="/file/a7c1559b7bba/dir/?style=paper">
  <a href="/file/a7c1559b7bba/foo?style=paper">

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/xyzzy/foo?style=paper' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=paper">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=paper">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=paper">changeset</a></li>
  <li><a href="/file/a7c1559b7bba/?style=paper">browse</a></li>
  <li><a href="/file/tip/foo?style=paper">latest</a></li>
  <li><a href="/diff/a7c1559b7bba/foo?style=paper">diff</a></li>
  <li><a href="/comparison/a7c1559b7bba/foo?style=paper">comparison</a></li>
  <li><a href="/annotate/a7c1559b7bba/foo?style=paper">annotate</a></li>
  <li><a href="/log/a7c1559b7bba/foo?style=paper">file log</a></li>
  <li><a href="/raw-file/a7c1559b7bba/foo">raw</a></li>
   <td class="author"><a href="/file/43c799df6e75/foo?style=paper">43c799df6e75</a> </td>
   <td class="author"><a href="/file/9d8c40cba617/foo?style=paper">9d8c40cba617</a> </td>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'log/xyzzy/foo?style=paper' | egrep $REVLINKS
     href="/atom-log/tip/foo" title="Atom feed for test:foo" />
     href="/rss-log/tip/foo" title="RSS feed for test:foo" />
  <li><a href="/shortlog/a7c1559b7bba?style=paper">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=paper">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=paper">changeset</a></li>
  <li><a href="/file/a7c1559b7bba?style=paper">browse</a></li>
  <li><a href="/file/a7c1559b7bba/foo?style=paper">file</a></li>
  <li><a href="/diff/a7c1559b7bba/foo?style=paper">diff</a></li>
  <li><a href="/comparison/a7c1559b7bba/foo?style=paper">comparison</a></li>
  <li><a href="/annotate/a7c1559b7bba/foo?style=paper">annotate</a></li>
  <li><a href="/raw-file/a7c1559b7bba/foo">raw</a></li>
  <a href="/atom-log/a7c1559b7bba/foo" title="subscribe to atom feed">
  <a href="/log/a7c1559b7bba/foo?revcount=30&style=paper">less</a>
  <a href="/log/a7c1559b7bba/foo?revcount=120&style=paper">more</a>
  | <a href="/log/43c799df6e75/foo?style=paper">(0)</a> <a href="/log/tip/foo?style=paper">tip</a> </div>
     <a href="/rev/a7c1559b7bba?style=paper">second</a>
     <a href="/rev/43c799df6e75?style=paper">first</a>
  <a href="/log/a7c1559b7bba/foo?revcount=30&style=paper">less</a>
  <a href="/log/a7c1559b7bba/foo?revcount=120&style=paper">more</a>
  | <a href="/log/43c799df6e75/foo?style=paper">(0)</a> <a href="/log/tip/foo?style=paper">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'annotate/xyzzy/foo?style=paper' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=paper">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=paper">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=paper">changeset</a></li>
  <li><a href="/file/a7c1559b7bba/?style=paper">browse</a></li>
  <li><a href="/file/a7c1559b7bba/foo?style=paper">file</a></li>
  <li><a href="/file/tip/foo?style=paper">latest</a></li>
  <li><a href="/diff/a7c1559b7bba/foo?style=paper">diff</a></li>
  <li><a href="/comparison/a7c1559b7bba/foo?style=paper">comparison</a></li>
  <li><a href="/log/a7c1559b7bba/foo?style=paper">file log</a></li>
  <li><a href="/raw-annotate/a7c1559b7bba/foo">raw</a></li>
   <td class="author"><a href="/file/43c799df6e75/foo?style=paper">43c799df6e75</a> </td>
   <td class="author"><a href="/file/9d8c40cba617/foo?style=paper">9d8c40cba617</a> </td>
  <a href="/annotate/43c799df6e75/foo?style=paper#l1"
  <a href="/annotate/a7c1559b7bba/foo?style=paper#l2"

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'diff/xyzzy/foo?style=paper' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=paper">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=paper">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=paper">changeset</a></li>
  <li><a href="/file/a7c1559b7bba?style=paper">browse</a></li>
  <li><a href="/file/a7c1559b7bba/foo?style=paper">file</a></li>
  <li><a href="/file/tip/foo?style=paper">latest</a></li>
  <li><a href="/comparison/a7c1559b7bba/foo?style=paper">comparison</a></li>
  <li><a href="/annotate/a7c1559b7bba/foo?style=paper">annotate</a></li>
  <li><a href="/log/a7c1559b7bba/foo?style=paper">file log</a></li>
  <li><a href="/raw-file/a7c1559b7bba/foo">raw</a></li>
   <td><a href="/file/43c799df6e75/foo?style=paper">43c799df6e75</a> </td>
   <td><a href="/file/9d8c40cba617/foo?style=paper">9d8c40cba617</a> </td>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'comparison/xyzzy/foo?style=paper' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=paper">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=paper">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=paper">changeset</a></li>
  <li><a href="/file/a7c1559b7bba?style=paper">browse</a></li>
  <li><a href="/file/a7c1559b7bba/foo?style=paper">file</a></li>
  <li><a href="/file/tip/foo?style=paper">latest</a></li>
  <li><a href="/diff/a7c1559b7bba/foo?style=paper">diff</a></li>
  <li><a href="/annotate/a7c1559b7bba/foo?style=paper">annotate</a></li>
  <li><a href="/log/a7c1559b7bba/foo?style=paper">file log</a></li>
  <li><a href="/raw-file/a7c1559b7bba/foo">raw</a></li>
   <td><a href="/file/43c799df6e75/foo?style=paper">43c799df6e75</a> </td>
   <td><a href="/file/9d8c40cba617/foo?style=paper">9d8c40cba617</a> </td>

(De)referencing symbolic revisions (coal)

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog?style=coal' | egrep $REVLINKS
  <li><a href="/graph/9d8c40cba617?style=coal">graph</a></li>
  <li><a href="/rev/9d8c40cba617?style=coal">changeset</a></li>
  <li><a href="/file/9d8c40cba617?style=coal">browse</a></li>
  <a href="/archive/tip.zip">zip</a>
  <a href="/shortlog/2?revcount=30&style=coal">less</a>
  <a href="/shortlog/2?revcount=120&style=coal">more</a>
  | rev 2: <a href="/shortlog/43c799df6e75?style=coal">(0)</a> <a href="/shortlog/tip?style=coal">tip</a> 
     <a href="/rev/9d8c40cba617?style=coal">third</a>
     <a href="/rev/a7c1559b7bba?style=coal">second</a>
     <a href="/rev/43c799df6e75?style=coal">first</a>
  <a href="/shortlog/2?revcount=30&style=coal">less</a>
  <a href="/shortlog/2?revcount=120&style=coal">more</a>
  | rev 2: <a href="/shortlog/43c799df6e75?style=coal">(0)</a> <a href="/shortlog/tip?style=coal">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'graph?style=coal' | egrep $REVLINKS
  <li><a href="/shortlog/9d8c40cba617?style=coal">log</a></li>
  <li><a href="/rev/9d8c40cba617?style=coal">changeset</a></li>
  <li><a href="/file/9d8c40cba617?style=coal">browse</a></li>
  <a href="/graph/2?revcount=30&style=coal">less</a>
  <a href="/graph/2?revcount=120&style=coal">more</a>
  | rev 2: <a href="/graph/43c799df6e75?style=coal">(0)</a> <a href="/graph/tip?style=coal">tip</a> 
  <a href="/graph/2?revcount=30&style=coal">less</a>
  <a href="/graph/2?revcount=120&style=coal">more</a>
  | rev 2: <a href="/graph/43c799df6e75?style=coal">(0)</a> <a href="/graph/tip?style=coal">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file?style=coal' | egrep $REVLINKS
  <li><a href="/shortlog/9d8c40cba617?style=coal">log</a></li>
  <li><a href="/graph/9d8c40cba617?style=coal">graph</a></li>
  <li><a href="/rev/9d8c40cba617?style=coal">changeset</a></li>
  <a href="/archive/9d8c40cba617.zip">zip</a>
    <td class="name"><a href="/file/9d8c40cba617/?style=coal">[up]</a></td>
  <a href="/file/9d8c40cba617/dir?style=coal">
  <a href="/file/9d8c40cba617/dir/?style=coal">
  <a href="/file/9d8c40cba617/foo?style=coal">

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog?style=coal&rev=all()' | egrep $REVLINKS
     <a href="/rev/9d8c40cba617?style=coal">third</a>
     <a href="/rev/a7c1559b7bba?style=coal">second</a>
     <a href="/rev/43c799df6e75?style=coal">first</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'rev/xyzzy?style=coal' | egrep $REVLINKS
   <li><a href="/shortlog/a7c1559b7bba?style=coal">log</a></li>
   <li><a href="/graph/a7c1559b7bba?style=coal">graph</a></li>
   <li><a href="/raw-rev/a7c1559b7bba?style=coal">raw</a></li>
   <li><a href="/file/a7c1559b7bba?style=coal">browse</a></li>
  <a href="/archive/a7c1559b7bba.zip">zip</a>
   <td class="author"><a href="/rev/43c799df6e75?style=coal">43c799df6e75</a> </td>
   <td class="author"> <a href="/rev/9d8c40cba617?style=coal">9d8c40cba617</a></td>
   <td class="files"><a href="/file/a7c1559b7bba/foo?style=coal">foo</a> </td>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog/xyzzy?style=coal' | egrep $REVLINKS
  <li><a href="/graph/a7c1559b7bba?style=coal">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=coal">changeset</a></li>
  <li><a href="/file/a7c1559b7bba?style=coal">browse</a></li>
  <a href="/archive/tip.zip">zip</a>
  <a href="/shortlog/1?revcount=30&style=coal">less</a>
  <a href="/shortlog/1?revcount=120&style=coal">more</a>
  | rev 1: <a href="/shortlog/43c799df6e75?style=coal">(0)</a> <a href="/shortlog/tip?style=coal">tip</a> 
     <a href="/rev/a7c1559b7bba?style=coal">second</a>
     <a href="/rev/43c799df6e75?style=coal">first</a>
  <a href="/shortlog/1?revcount=30&style=coal">less</a>
  <a href="/shortlog/1?revcount=120&style=coal">more</a>
  | rev 1: <a href="/shortlog/43c799df6e75?style=coal">(0)</a> <a href="/shortlog/tip?style=coal">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'graph/xyzzy?style=coal' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=coal">log</a></li>
  <li><a href="/rev/a7c1559b7bba?style=coal">changeset</a></li>
  <li><a href="/file/a7c1559b7bba?style=coal">browse</a></li>
  <a href="/graph/1?revcount=30&style=coal">less</a>
  <a href="/graph/1?revcount=120&style=coal">more</a>
  | rev 1: <a href="/graph/43c799df6e75?style=coal">(0)</a> <a href="/graph/tip?style=coal">tip</a> 
  <a href="/graph/1?revcount=30&style=coal">less</a>
  <a href="/graph/1?revcount=120&style=coal">more</a>
  | rev 1: <a href="/graph/43c799df6e75?style=coal">(0)</a> <a href="/graph/tip?style=coal">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/xyzzy?style=coal' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=coal">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=coal">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=coal">changeset</a></li>
  <a href="/archive/a7c1559b7bba.zip">zip</a>
    <td class="name"><a href="/file/a7c1559b7bba/?style=coal">[up]</a></td>
  <a href="/file/a7c1559b7bba/dir?style=coal">
  <a href="/file/a7c1559b7bba/dir/?style=coal">
  <a href="/file/a7c1559b7bba/foo?style=coal">

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/xyzzy/foo?style=coal' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=coal">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=coal">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=coal">changeset</a></li>
  <li><a href="/file/a7c1559b7bba/?style=coal">browse</a></li>
  <li><a href="/file/tip/foo?style=coal">latest</a></li>
  <li><a href="/diff/a7c1559b7bba/foo?style=coal">diff</a></li>
  <li><a href="/comparison/a7c1559b7bba/foo?style=coal">comparison</a></li>
  <li><a href="/annotate/a7c1559b7bba/foo?style=coal">annotate</a></li>
  <li><a href="/log/a7c1559b7bba/foo?style=coal">file log</a></li>
  <li><a href="/raw-file/a7c1559b7bba/foo">raw</a></li>
   <td class="author"><a href="/file/43c799df6e75/foo?style=coal">43c799df6e75</a> </td>
   <td class="author"><a href="/file/9d8c40cba617/foo?style=coal">9d8c40cba617</a> </td>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'log/xyzzy/foo?style=coal' | egrep $REVLINKS
     href="/atom-log/tip/foo" title="Atom feed for test:foo" />
     href="/rss-log/tip/foo" title="RSS feed for test:foo" />
  <li><a href="/shortlog/a7c1559b7bba?style=coal">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=coal">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=coal">changeset</a></li>
  <li><a href="/file/a7c1559b7bba?style=coal">browse</a></li>
  <li><a href="/file/a7c1559b7bba/foo?style=coal">file</a></li>
  <li><a href="/diff/a7c1559b7bba/foo?style=coal">diff</a></li>
  <li><a href="/comparison/a7c1559b7bba/foo?style=coal">comparison</a></li>
  <li><a href="/annotate/a7c1559b7bba/foo?style=coal">annotate</a></li>
  <li><a href="/raw-file/a7c1559b7bba/foo">raw</a></li>
  <a href="/atom-log/a7c1559b7bba/foo" title="subscribe to atom feed">
  <a href="/log/a7c1559b7bba/foo?revcount=30&style=coal">less</a>
  <a href="/log/a7c1559b7bba/foo?revcount=120&style=coal">more</a>
  | <a href="/log/43c799df6e75/foo?style=coal">(0)</a> <a href="/log/tip/foo?style=coal">tip</a> </div>
     <a href="/rev/a7c1559b7bba?style=coal">second</a>
     <a href="/rev/43c799df6e75?style=coal">first</a>
  <a href="/log/a7c1559b7bba/foo?revcount=30&style=coal">less</a>
  <a href="/log/a7c1559b7bba/foo?revcount=120&style=coal">more</a>
  | <a href="/log/43c799df6e75/foo?style=coal">(0)</a> <a href="/log/tip/foo?style=coal">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'annotate/xyzzy/foo?style=coal' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=coal">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=coal">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=coal">changeset</a></li>
  <li><a href="/file/a7c1559b7bba/?style=coal">browse</a></li>
  <li><a href="/file/a7c1559b7bba/foo?style=coal">file</a></li>
  <li><a href="/file/tip/foo?style=coal">latest</a></li>
  <li><a href="/diff/a7c1559b7bba/foo?style=coal">diff</a></li>
  <li><a href="/comparison/a7c1559b7bba/foo?style=coal">comparison</a></li>
  <li><a href="/log/a7c1559b7bba/foo?style=coal">file log</a></li>
  <li><a href="/raw-annotate/a7c1559b7bba/foo">raw</a></li>
   <td class="author"><a href="/file/43c799df6e75/foo?style=coal">43c799df6e75</a> </td>
   <td class="author"><a href="/file/9d8c40cba617/foo?style=coal">9d8c40cba617</a> </td>
  <a href="/annotate/43c799df6e75/foo?style=coal#1"
  <a href="/annotate/a7c1559b7bba/foo?style=coal#2"

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'diff/xyzzy/foo?style=coal' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=coal">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=coal">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=coal">changeset</a></li>
  <li><a href="/file/a7c1559b7bba?style=coal">browse</a></li>
  <li><a href="/file/a7c1559b7bba/foo?style=coal">file</a></li>
  <li><a href="/file/tip/foo?style=coal">latest</a></li>
  <li><a href="/comparison/a7c1559b7bba/foo?style=coal">comparison</a></li>
  <li><a href="/annotate/a7c1559b7bba/foo?style=coal">annotate</a></li>
  <li><a href="/log/a7c1559b7bba/foo?style=coal">file log</a></li>
  <li><a href="/raw-file/a7c1559b7bba/foo">raw</a></li>
   <td><a href="/file/43c799df6e75/foo?style=coal">43c799df6e75</a> </td>
   <td><a href="/file/9d8c40cba617/foo?style=coal">9d8c40cba617</a> </td>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'comparison/xyzzy/foo?style=coal' | egrep $REVLINKS
  <li><a href="/shortlog/a7c1559b7bba?style=coal">log</a></li>
  <li><a href="/graph/a7c1559b7bba?style=coal">graph</a></li>
  <li><a href="/rev/a7c1559b7bba?style=coal">changeset</a></li>
  <li><a href="/file/a7c1559b7bba?style=coal">browse</a></li>
  <li><a href="/file/a7c1559b7bba/foo?style=coal">file</a></li>
  <li><a href="/file/tip/foo?style=coal">latest</a></li>
  <li><a href="/diff/a7c1559b7bba/foo?style=coal">diff</a></li>
  <li><a href="/annotate/a7c1559b7bba/foo?style=coal">annotate</a></li>
  <li><a href="/log/a7c1559b7bba/foo?style=coal">file log</a></li>
  <li><a href="/raw-file/a7c1559b7bba/foo">raw</a></li>
   <td><a href="/file/43c799df6e75/foo?style=coal">43c799df6e75</a> </td>
   <td><a href="/file/9d8c40cba617/foo?style=coal">9d8c40cba617</a> </td>

(De)referencing symbolic revisions (gitweb)

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'summary?style=gitweb' | egrep $REVLINKS
  <a href="/file?style=gitweb">files</a> | <a href="/archive/tip.zip">zip</a>  |
  <a class="list" href="/rev/9d8c40cba617?style=gitweb">
  <a href="/rev/9d8c40cba617?style=gitweb">changeset</a> |
  <a href="/file/9d8c40cba617?style=gitweb">files</a>
  <a class="list" href="/rev/a7c1559b7bba?style=gitweb">
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a> |
  <a href="/file/a7c1559b7bba?style=gitweb">files</a>
  <a class="list" href="/rev/43c799df6e75?style=gitweb">
  <a href="/rev/43c799df6e75?style=gitweb">changeset</a> |
  <a href="/file/43c799df6e75?style=gitweb">files</a>
  <td><a class="list" href="/rev/a7c1559b7bba?style=gitweb"><b>xyzzy</b></a></td>
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a> |
  <a href="/log/a7c1559b7bba?style=gitweb">changelog</a> |
  <a href="/file/a7c1559b7bba?style=gitweb">files</a>
  <td><a class="list" href="/shortlog/9d8c40cba617?style=gitweb"><b>9d8c40cba617</b></a></td>
  <a href="/changeset/9d8c40cba617?style=gitweb">changeset</a> |
  <a href="/log/9d8c40cba617?style=gitweb">changelog</a> |
  <a href="/file/9d8c40cba617?style=gitweb">files</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog?style=gitweb' | egrep $REVLINKS
  <a href="/log/2?style=gitweb">changelog</a> |
  <a href="/file/9d8c40cba617?style=gitweb">files</a> | <a href="/archive/tip.zip">zip</a>  |
  <br/><a href="/shortlog/43c799df6e75?style=gitweb">(0)</a> <a href="/shortlog/tip?style=gitweb">tip</a> <br/>
  <a class="list" href="/rev/9d8c40cba617?style=gitweb">
  <a href="/rev/9d8c40cba617?style=gitweb">changeset</a> |
  <a href="/file/9d8c40cba617?style=gitweb">files</a>
  <a class="list" href="/rev/a7c1559b7bba?style=gitweb">
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a> |
  <a href="/file/a7c1559b7bba?style=gitweb">files</a>
  <a class="list" href="/rev/43c799df6e75?style=gitweb">
  <a href="/rev/43c799df6e75?style=gitweb">changeset</a> |
  <a href="/file/43c799df6e75?style=gitweb">files</a>
  <a href="/shortlog/43c799df6e75?style=gitweb">(0)</a> <a href="/shortlog/tip?style=gitweb">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'log?style=gitweb' | egrep $REVLINKS
  <a href="/shortlog/2?style=gitweb">shortlog</a> |
  <a href="/file/9d8c40cba617?style=gitweb">files</a> | <a href="/archive/tip.zip">zip</a>  |
  <a href="/log/43c799df6e75?style=gitweb">(0)</a>  <a href="/log/tip?style=gitweb">tip</a> <br/>
  <a class="title" href="/rev/9d8c40cba617?style=gitweb"><span class="age">Thu, 01 Jan 1970 00:00:00 +0000</span>third<span class="logtags"> <span class="branchtag" title="default">default</span> <span class="tagtag" title="tip">tip</span> </span></a>
  <a href="/rev/9d8c40cba617?style=gitweb">changeset</a><br/>
  <a class="title" href="/rev/a7c1559b7bba?style=gitweb"><span class="age">Thu, 01 Jan 1970 00:00:00 +0000</span>second<span class="logtags"> <span class="bookmarktag" title="xyzzy">xyzzy</span> </span></a>
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a><br/>
  <a class="title" href="/rev/43c799df6e75?style=gitweb"><span class="age">Thu, 01 Jan 1970 00:00:00 +0000</span>first<span class="logtags"> </span></a>
  <a href="/rev/43c799df6e75?style=gitweb">changeset</a><br/>
  <a href="/log/43c799df6e75?style=gitweb">(0)</a>  <a href="/log/tip?style=gitweb">tip</a> <br/>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'graph?style=gitweb' | egrep $REVLINKS
  <a href="/log/2?style=gitweb">changelog</a> |
  <a href="/file/9d8c40cba617?style=gitweb">files</a> |
  <a href="/graph/2?revcount=30&style=gitweb">less</a>
  <a href="/graph/2?revcount=120&style=gitweb">more</a>
  | <a href="/graph/43c799df6e75?style=gitweb">(0)</a> <a href="/graph/tip?style=gitweb">tip</a> <br/>
  <a href="/graph/2?revcount=30&style=gitweb">less</a>
  <a href="/graph/2?revcount=120&style=gitweb">more</a>
  | <a href="/graph/43c799df6e75?style=gitweb">(0)</a> <a href="/graph/tip?style=gitweb">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'tags?style=gitweb' | egrep $REVLINKS
  <td><a class="list" href="/rev/9d8c40cba617?style=gitweb"><b>tip</b></a></td>
  <a href="/rev/9d8c40cba617?style=gitweb">changeset</a> |
  <a href="/log/9d8c40cba617?style=gitweb">changelog</a> |
  <a href="/file/9d8c40cba617?style=gitweb">files</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'bookmarks?style=gitweb' | egrep $REVLINKS
  <td><a class="list" href="/rev/a7c1559b7bba?style=gitweb"><b>xyzzy</b></a></td>
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a> |
  <a href="/log/a7c1559b7bba?style=gitweb">changelog</a> |
  <a href="/file/a7c1559b7bba?style=gitweb">files</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'branches?style=gitweb' | egrep $REVLINKS
  <td><a class="list" href="/shortlog/9d8c40cba617?style=gitweb"><b>9d8c40cba617</b></a></td>
  <a href="/changeset/9d8c40cba617?style=gitweb">changeset</a> |
  <a href="/log/9d8c40cba617?style=gitweb">changelog</a> |
  <a href="/file/9d8c40cba617?style=gitweb">files</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file?style=gitweb' | egrep $REVLINKS
  <a href="/rev/9d8c40cba617?style=gitweb">changeset</a>  | <a href="/archive/9d8c40cba617.zip">zip</a>  |
  <td><a href="/file/9d8c40cba617/?style=gitweb">[up]</a></td>
  <a href="/file/9d8c40cba617/dir?style=gitweb">dir</a>
  <a href="/file/9d8c40cba617/dir/?style=gitweb"></a>
  <a href="/file/9d8c40cba617/dir?style=gitweb">files</a>
  <a class="list" href="/file/9d8c40cba617/foo?style=gitweb">foo</a>
  <a href="/file/9d8c40cba617/foo?style=gitweb">file</a> |
  <a href="/log/9d8c40cba617/foo?style=gitweb">revisions</a> |
  <a href="/annotate/9d8c40cba617/foo?style=gitweb">annotate</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog?style=gitweb&rev=all()' | egrep $REVLINKS
  <a href="/file?style=gitweb">files</a> | <a href="/archive/tip.zip">zip</a> 
  <a class="title" href="/rev/9d8c40cba617?style=gitweb"><span class="age">Thu, 01 Jan 1970 00:00:00 +0000</span>third<span class="logtags"> <span class="branchtag" title="default">default</span> <span class="tagtag" title="tip">tip</span> </span></a>
  <a href="/rev/9d8c40cba617?style=gitweb">changeset</a><br/>
  <a class="title" href="/rev/a7c1559b7bba?style=gitweb"><span class="age">Thu, 01 Jan 1970 00:00:00 +0000</span>second<span class="logtags"> <span class="bookmarktag" title="xyzzy">xyzzy</span> </span></a>
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a><br/>
  <a class="title" href="/rev/43c799df6e75?style=gitweb"><span class="age">Thu, 01 Jan 1970 00:00:00 +0000</span>first<span class="logtags"> </span></a>
  <a href="/rev/43c799df6e75?style=gitweb">changeset</a><br/>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'rev/xyzzy?style=gitweb' | egrep $REVLINKS
  <a href="/shortlog/1?style=gitweb">shortlog</a> |
  <a href="/log/1?style=gitweb">changelog</a> |
  <a href="/file/a7c1559b7bba?style=gitweb">files</a> |
  <a href="/raw-rev/a7c1559b7bba">raw</a>  | <a href="/archive/a7c1559b7bba.zip">zip</a>  |
  <a class="title" href="/raw-rev/a7c1559b7bba">second <span class="logtags"><span class="bookmarktag" title="xyzzy">xyzzy</span> </span></a>
  <a class="list" href="/rev/43c799df6e75?style=gitweb">43c799df6e75</a>
  <a class="list" href="/rev/9d8c40cba617?style=gitweb">9d8c40cba617</a>
  <td><a class="list" href="/diff/a7c1559b7bba/foo?style=gitweb">foo</a></td>
  <a href="/file/a7c1559b7bba/foo?style=gitweb">file</a> |
  <a href="/annotate/a7c1559b7bba/foo?style=gitweb">annotate</a> |
  <a href="/diff/a7c1559b7bba/foo?style=gitweb">diff</a> |
  <a href="/comparison/a7c1559b7bba/foo?style=gitweb">comparison</a> |
  <a href="/log/a7c1559b7bba/foo?style=gitweb">revisions</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog/xyzzy?style=gitweb' | egrep $REVLINKS
  <a href="/log/1?style=gitweb">changelog</a> |
  <a href="/file/a7c1559b7bba?style=gitweb">files</a> | <a href="/archive/tip.zip">zip</a>  |
  <br/><a href="/shortlog/43c799df6e75?style=gitweb">(0)</a> <a href="/shortlog/tip?style=gitweb">tip</a> <br/>
  <a class="list" href="/rev/a7c1559b7bba?style=gitweb">
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a> |
  <a href="/file/a7c1559b7bba?style=gitweb">files</a>
  <a class="list" href="/rev/43c799df6e75?style=gitweb">
  <a href="/rev/43c799df6e75?style=gitweb">changeset</a> |
  <a href="/file/43c799df6e75?style=gitweb">files</a>
  <a href="/shortlog/43c799df6e75?style=gitweb">(0)</a> <a href="/shortlog/tip?style=gitweb">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'log/xyzzy?style=gitweb' | egrep $REVLINKS
  <a href="/shortlog/1?style=gitweb">shortlog</a> |
  <a href="/file/a7c1559b7bba?style=gitweb">files</a> | <a href="/archive/tip.zip">zip</a>  |
  <a href="/log/43c799df6e75?style=gitweb">(0)</a>  <a href="/log/tip?style=gitweb">tip</a> <br/>
  <a class="title" href="/rev/a7c1559b7bba?style=gitweb"><span class="age">Thu, 01 Jan 1970 00:00:00 +0000</span>second<span class="logtags"> <span class="bookmarktag" title="xyzzy">xyzzy</span> </span></a>
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a><br/>
  <a class="title" href="/rev/43c799df6e75?style=gitweb"><span class="age">Thu, 01 Jan 1970 00:00:00 +0000</span>first<span class="logtags"> </span></a>
  <a href="/rev/43c799df6e75?style=gitweb">changeset</a><br/>
  <a href="/log/43c799df6e75?style=gitweb">(0)</a>  <a href="/log/tip?style=gitweb">tip</a> <br/>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'graph/xyzzy?style=gitweb' | egrep $REVLINKS
  <a href="/log/1?style=gitweb">changelog</a> |
  <a href="/file/a7c1559b7bba?style=gitweb">files</a> |
  <a href="/graph/1?revcount=30&style=gitweb">less</a>
  <a href="/graph/1?revcount=120&style=gitweb">more</a>
  | <a href="/graph/43c799df6e75?style=gitweb">(0)</a> <a href="/graph/tip?style=gitweb">tip</a> <br/>
  <a href="/graph/1?revcount=30&style=gitweb">less</a>
  <a href="/graph/1?revcount=120&style=gitweb">more</a>
  | <a href="/graph/43c799df6e75?style=gitweb">(0)</a> <a href="/graph/tip?style=gitweb">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/xyzzy?style=gitweb' | egrep $REVLINKS
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a>  | <a href="/archive/a7c1559b7bba.zip">zip</a>  |
  <td><a href="/file/a7c1559b7bba/?style=gitweb">[up]</a></td>
  <a href="/file/a7c1559b7bba/dir?style=gitweb">dir</a>
  <a href="/file/a7c1559b7bba/dir/?style=gitweb"></a>
  <a href="/file/a7c1559b7bba/dir?style=gitweb">files</a>
  <a class="list" href="/file/a7c1559b7bba/foo?style=gitweb">foo</a>
  <a href="/file/a7c1559b7bba/foo?style=gitweb">file</a> |
  <a href="/log/a7c1559b7bba/foo?style=gitweb">revisions</a> |
  <a href="/annotate/a7c1559b7bba/foo?style=gitweb">annotate</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/xyzzy/foo?style=gitweb' | egrep $REVLINKS
  <a href="/file/a7c1559b7bba/?style=gitweb">files</a> |
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a> |
  <a href="/file/tip/foo?style=gitweb">latest</a> |
  <a href="/log/a7c1559b7bba/foo?style=gitweb">revisions</a> |
  <a href="/annotate/a7c1559b7bba/foo?style=gitweb">annotate</a> |
  <a href="/diff/a7c1559b7bba/foo?style=gitweb">diff</a> |
  <a href="/comparison/a7c1559b7bba/foo?style=gitweb">comparison</a> |
  <a href="/raw-file/a7c1559b7bba/foo">raw</a> |
   <td style="font-family:monospace"><a class="list" href="/rev/a7c1559b7bba?style=gitweb">a7c1559b7bba</a></td>
  <a class="list" href="/file/43c799df6e75/foo?style=gitweb">
  <a class="list" href="/file/9d8c40cba617/foo?style=gitweb">9d8c40cba617</a></td>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'log/xyzzy/foo?style=gitweb' | egrep $REVLINKS
  <a href="/file/a7c1559b7bba/foo?style=gitweb">file</a> |
  <a href="/annotate/a7c1559b7bba/foo?style=gitweb">annotate</a> |
  <a href="/diff/a7c1559b7bba/foo?style=gitweb">diff</a> |
  <a href="/comparison/a7c1559b7bba/foo?style=gitweb">comparison</a> |
  <a href="/rss-log/tip/foo">rss</a> |
  <a href="/log/43c799df6e75/foo?style=gitweb">(0)</a> <a href="/log/tip/foo?style=gitweb">tip</a> 
  <a class="list" href="/rev/a7c1559b7bba?style=gitweb">
  <a href="/file/a7c1559b7bba/foo?style=gitweb">file</a> |
  <a href="/diff/a7c1559b7bba/foo?style=gitweb">diff</a> |
  <a href="/annotate/a7c1559b7bba/foo?style=gitweb">annotate</a>
  <a class="list" href="/rev/43c799df6e75?style=gitweb">
  <a href="/file/43c799df6e75/foo?style=gitweb">file</a> |
  <a href="/diff/43c799df6e75/foo?style=gitweb">diff</a> |
  <a href="/annotate/43c799df6e75/foo?style=gitweb">annotate</a>
  <a href="/log/43c799df6e75/foo?style=gitweb">(0)</a> <a href="/log/tip/foo?style=gitweb">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'annotate/xyzzy/foo?style=gitweb' | egrep $REVLINKS
  <a href="/file/a7c1559b7bba/?style=gitweb">files</a> |
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a> |
  <a href="/file/a7c1559b7bba/foo?style=gitweb">file</a> |
  <a href="/file/tip/foo?style=gitweb">latest</a> |
  <a href="/log/a7c1559b7bba/foo?style=gitweb">revisions</a> |
  <a href="/diff/a7c1559b7bba/foo?style=gitweb">diff</a> |
  <a href="/comparison/a7c1559b7bba/foo?style=gitweb">comparison</a> |
  <a href="/raw-annotate/a7c1559b7bba/foo">raw</a> |
   <td style="font-family:monospace"><a class="list" href="/rev/a7c1559b7bba?style=gitweb">a7c1559b7bba</a></td>
  <a class="list" href="/annotate/43c799df6e75/foo?style=gitweb">
  <a class="list" href="/annotate/9d8c40cba617/foo?style=gitweb">9d8c40cba617</a></td>
  <a href="/annotate/43c799df6e75/foo?style=gitweb#l1"
  <a href="/annotate/a7c1559b7bba/foo?style=gitweb#l2"

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'diff/xyzzy/foo?style=gitweb' | egrep $REVLINKS
  <a href="/file/a7c1559b7bba?style=gitweb">files</a> |
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a> |
  <a href="/file/a7c1559b7bba/foo?style=gitweb">file</a> |
  <a href="/file/tip/foo?style=gitweb">latest</a> |
  <a href="/log/a7c1559b7bba/foo?style=gitweb">revisions</a> |
  <a href="/annotate/a7c1559b7bba/foo?style=gitweb">annotate</a> |
  <a href="/comparison/a7c1559b7bba/foo?style=gitweb">comparison</a> |
  <a href="/raw-diff/a7c1559b7bba/foo">raw</a> |
   <td style="font-family:monospace"><a class="list" href="/rev/a7c1559b7bba?style=gitweb">a7c1559b7bba</a></td>
  <a class="list" href="/diff/43c799df6e75/foo?style=gitweb">
  <a class="list" href="/diff/9d8c40cba617/foo?style=gitweb">9d8c40cba617</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'comparison/xyzzy/foo?style=gitweb' | egrep $REVLINKS
  <a href="/file/a7c1559b7bba?style=gitweb">files</a> |
  <a href="/rev/a7c1559b7bba?style=gitweb">changeset</a> |
  <a href="/file/a7c1559b7bba/foo?style=gitweb">file</a> |
  <a href="/file/tip/foo?style=gitweb">latest</a> |
  <a href="/log/a7c1559b7bba/foo?style=gitweb">revisions</a> |
  <a href="/annotate/a7c1559b7bba/foo?style=gitweb">annotate</a> |
  <a href="/diff/a7c1559b7bba/foo?style=gitweb">diff</a> |
  <a href="/raw-diff/a7c1559b7bba/foo">raw</a> |
   <td style="font-family:monospace"><a class="list" href="/rev/a7c1559b7bba?style=gitweb">a7c1559b7bba</a></td>
  <a class="list" href="/comparison/43c799df6e75/foo?style=gitweb">
  <a class="list" href="/comparison/9d8c40cba617/foo?style=gitweb">9d8c40cba617</a>

(De)referencing symbolic revisions (monoblue)

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'summary?style=monoblue' | egrep $REVLINKS
  <a href="/rev/9d8c40cba617?style=monoblue">
  <a href="/rev/9d8c40cba617?style=monoblue">changeset</a> |
  <a href="/file/9d8c40cba617?style=monoblue">files</a>
  <a href="/rev/a7c1559b7bba?style=monoblue">
  <a href="/rev/a7c1559b7bba?style=monoblue">changeset</a> |
  <a href="/file/a7c1559b7bba?style=monoblue">files</a>
  <a href="/rev/43c799df6e75?style=monoblue">
  <a href="/rev/43c799df6e75?style=monoblue">changeset</a> |
  <a href="/file/43c799df6e75?style=monoblue">files</a>
  <td><a href="/rev/a7c1559b7bba?style=monoblue">xyzzy</a></td>
  <a href="/rev/a7c1559b7bba?style=monoblue">changeset</a> |
  <a href="/log/a7c1559b7bba?style=monoblue">changelog</a> |
  <a href="/file/a7c1559b7bba?style=monoblue">files</a>
  <td><a href="/shortlog/9d8c40cba617?style=monoblue">9d8c40cba617</a></td>
  <a href="/rev/9d8c40cba617?style=monoblue">changeset</a> |
  <a href="/log/9d8c40cba617?style=monoblue">changelog</a> |
  <a href="/file/9d8c40cba617?style=monoblue">files</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/9d8c40cba617?style=monoblue">graph</a></li>
              <li><a href="/file/9d8c40cba617?style=monoblue">files</a></li>
              <li><a href="/archive/tip.zip">zip</a></li>
  <a href="/rev/9d8c40cba617?style=monoblue">
  <a href="/rev/9d8c40cba617?style=monoblue">changeset</a> |
  <a href="/file/9d8c40cba617?style=monoblue">files</a>
  <a href="/rev/a7c1559b7bba?style=monoblue">
  <a href="/rev/a7c1559b7bba?style=monoblue">changeset</a> |
  <a href="/file/a7c1559b7bba?style=monoblue">files</a>
  <a href="/rev/43c799df6e75?style=monoblue">
  <a href="/rev/43c799df6e75?style=monoblue">changeset</a> |
  <a href="/file/43c799df6e75?style=monoblue">files</a>
      <a href="/shortlog/43c799df6e75?style=monoblue">(0)</a> <a href="/shortlog/tip?style=monoblue">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'log?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/9d8c40cba617?style=monoblue">graph</a></li>
              <li><a href="/file/9d8c40cba617?style=monoblue">files</a></li>
              <li><a href="/archive/tip.zip">zip</a></li>
      <h3 class="changelog"><a class="title" href="/rev/9d8c40cba617?style=monoblue">third<span class="logtags"> <span class="branchtag" title="default">default</span> <span class="tagtag" title="tip">tip</span> </span></a></h3>
  <h3 class="changelog"><a class="title" href="/rev/a7c1559b7bba?style=monoblue">second<span class="logtags"> <span class="bookmarktag" title="xyzzy">xyzzy</span> </span></a></h3>
  <h3 class="changelog"><a class="title" href="/rev/43c799df6e75?style=monoblue">first<span class="logtags"> </span></a></h3>
  <a href="/log/43c799df6e75?style=monoblue">(0)</a>  <a href="/log/tip?style=monoblue">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'graph?style=monoblue' | egrep $REVLINKS
              <li><a href="/file/9d8c40cba617?style=monoblue">files</a></li>
          <a href="/graph/2?revcount=30&style=monoblue">less</a>
          <a href="/graph/2?revcount=120&style=monoblue">more</a>
          | <a href="/graph/43c799df6e75?style=monoblue">(0)</a> <a href="/graph/tip?style=monoblue">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'tags?style=monoblue' | egrep $REVLINKS
  <td><a href="/rev/9d8c40cba617?style=monoblue">tip</a></td>
  <a href="/rev/9d8c40cba617?style=monoblue">changeset</a> |
  <a href="/log/9d8c40cba617?style=monoblue">changelog</a> |
  <a href="/file/9d8c40cba617?style=monoblue">files</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'bookmarks?style=monoblue' | egrep $REVLINKS
  <td><a href="/rev/a7c1559b7bba?style=monoblue">xyzzy</a></td>
  <a href="/rev/a7c1559b7bba?style=monoblue">changeset</a> |
  <a href="/log/a7c1559b7bba?style=monoblue">changelog</a> |
  <a href="/file/a7c1559b7bba?style=monoblue">files</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'branches?style=monoblue' | egrep $REVLINKS
  <td><a href="/shortlog/9d8c40cba617?style=monoblue">9d8c40cba617</a></td>
  <a href="/rev/9d8c40cba617?style=monoblue">changeset</a> |
  <a href="/log/9d8c40cba617?style=monoblue">changelog</a> |
  <a href="/file/9d8c40cba617?style=monoblue">files</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/9d8c40cba617?style=monoblue">graph</a></li>
          <li><a href="/rev/9d8c40cba617?style=monoblue">changeset</a></li>
          <li><a href="/archive/9d8c40cba617.zip">zip</a></li>
              <td><a href="/file/9d8c40cba617/?style=monoblue">[up]</a></td>
  <a href="/file/9d8c40cba617/dir?style=monoblue">dir</a>
  <a href="/file/9d8c40cba617/dir/?style=monoblue"></a>
  <td><a href="/file/9d8c40cba617/dir?style=monoblue">files</a></td>
  <td><a href="/file/9d8c40cba617/foo?style=monoblue">foo</a></td>
  <a href="/file/9d8c40cba617/foo?style=monoblue">file</a> |
  <a href="/log/9d8c40cba617/foo?style=monoblue">revisions</a> |
  <a href="/annotate/9d8c40cba617/foo?style=monoblue">annotate</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog?style=monoblue&rev=all()' | egrep $REVLINKS
              <li><a href="/archive/tip.zip">zip</a></li>
      <h3 class="changelog"><a class="title" href="/rev/9d8c40cba617?style=monoblue">third<span class="logtags"> <span class="branchtag" title="default">default</span> <span class="tagtag" title="tip">tip</span> </span></a></h3>
  <h3 class="changelog"><a class="title" href="/rev/a7c1559b7bba?style=monoblue">second<span class="logtags"> <span class="bookmarktag" title="xyzzy">xyzzy</span> </span></a></h3>
  <h3 class="changelog"><a class="title" href="/rev/43c799df6e75?style=monoblue">first<span class="logtags"> </span></a></h3>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'rev/xyzzy?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/a7c1559b7bba?style=monoblue">graph</a></li>
              <li><a href="/file/a7c1559b7bba?style=monoblue">files</a></li>
          <li><a href="/raw-rev/a7c1559b7bba">raw</a></li>
          <li><a href="/archive/a7c1559b7bba.zip">zip</a></li>
      <h3 class="changeset"><a href="/raw-rev/a7c1559b7bba">second <span class="logtags"><span class="bookmarktag" title="xyzzy">xyzzy</span> </span></a></h3>
  <dd><a href="/rev/43c799df6e75?style=monoblue">43c799df6e75</a></dd>
  <dd><a href="/rev/9d8c40cba617?style=monoblue">9d8c40cba617</a></dd>
  <td><a href="/diff/a7c1559b7bba/foo?style=monoblue">foo</a></td>
  <a href="/file/a7c1559b7bba/foo?style=monoblue">file</a> |
  <a href="/annotate/a7c1559b7bba/foo?style=monoblue">annotate</a> |
  <a href="/diff/a7c1559b7bba/foo?style=monoblue">diff</a> |
  <a href="/comparison/a7c1559b7bba/foo?style=monoblue">comparison</a> |
  <a href="/log/a7c1559b7bba/foo?style=monoblue">revisions</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog/xyzzy?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/a7c1559b7bba?style=monoblue">graph</a></li>
              <li><a href="/file/a7c1559b7bba?style=monoblue">files</a></li>
              <li><a href="/archive/tip.zip">zip</a></li>
  <a href="/rev/a7c1559b7bba?style=monoblue">
  <a href="/rev/a7c1559b7bba?style=monoblue">changeset</a> |
  <a href="/file/a7c1559b7bba?style=monoblue">files</a>
  <a href="/rev/43c799df6e75?style=monoblue">
  <a href="/rev/43c799df6e75?style=monoblue">changeset</a> |
  <a href="/file/43c799df6e75?style=monoblue">files</a>
      <a href="/shortlog/43c799df6e75?style=monoblue">(0)</a> <a href="/shortlog/tip?style=monoblue">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'log/xyzzy?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/a7c1559b7bba?style=monoblue">graph</a></li>
              <li><a href="/file/a7c1559b7bba?style=monoblue">files</a></li>
              <li><a href="/archive/tip.zip">zip</a></li>
      <h3 class="changelog"><a class="title" href="/rev/a7c1559b7bba?style=monoblue">second<span class="logtags"> <span class="bookmarktag" title="xyzzy">xyzzy</span> </span></a></h3>
  <h3 class="changelog"><a class="title" href="/rev/43c799df6e75?style=monoblue">first<span class="logtags"> </span></a></h3>
  <a href="/log/43c799df6e75?style=monoblue">(0)</a>  <a href="/log/tip?style=monoblue">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'graph/xyzzy?style=monoblue' | egrep $REVLINKS
              <li><a href="/file/a7c1559b7bba?style=monoblue">files</a></li>
          <a href="/graph/1?revcount=30&style=monoblue">less</a>
          <a href="/graph/1?revcount=120&style=monoblue">more</a>
          | <a href="/graph/43c799df6e75?style=monoblue">(0)</a> <a href="/graph/tip?style=monoblue">tip</a> 

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/xyzzy?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/a7c1559b7bba?style=monoblue">graph</a></li>
          <li><a href="/rev/a7c1559b7bba?style=monoblue">changeset</a></li>
          <li><a href="/archive/a7c1559b7bba.zip">zip</a></li>
              <td><a href="/file/a7c1559b7bba/?style=monoblue">[up]</a></td>
  <a href="/file/a7c1559b7bba/dir?style=monoblue">dir</a>
  <a href="/file/a7c1559b7bba/dir/?style=monoblue"></a>
  <td><a href="/file/a7c1559b7bba/dir?style=monoblue">files</a></td>
  <td><a href="/file/a7c1559b7bba/foo?style=monoblue">foo</a></td>
  <a href="/file/a7c1559b7bba/foo?style=monoblue">file</a> |
  <a href="/log/a7c1559b7bba/foo?style=monoblue">revisions</a> |
  <a href="/annotate/a7c1559b7bba/foo?style=monoblue">annotate</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/xyzzy/foo?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/a7c1559b7bba?style=monoblue">graph</a></li>
              <li><a href="/file/a7c1559b7bba/?style=monoblue">files</a></li>
          <li><a href="/log/a7c1559b7bba/foo?style=monoblue">revisions</a></li>
          <li><a href="/annotate/a7c1559b7bba/foo?style=monoblue">annotate</a></li>
          <li><a href="/diff/a7c1559b7bba/foo?style=monoblue">diff</a></li>
          <li><a href="/comparison/a7c1559b7bba/foo?style=monoblue">comparison</a></li>
          <li><a href="/raw-file/a7c1559b7bba/foo">raw</a></li>
          <dd><a class="list" href="/rev/a7c1559b7bba?style=monoblue">a7c1559b7bba</a></dd>
  <a href="/file/43c799df6e75/foo?style=monoblue">
  <a href="/file/9d8c40cba617/foo?style=monoblue">9d8c40cba617</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'log/xyzzy/foo?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/a7c1559b7bba?style=monoblue">graph</a></li>
              <li><a href="/file/a7c1559b7bba?style=monoblue">files</a></li>
          <li><a href="/file/a7c1559b7bba/foo?style=monoblue">file</a></li>
          <li><a href="/annotate/a7c1559b7bba/foo?style=monoblue">annotate</a></li>
          <li><a href="/diff/a7c1559b7bba/foo?style=monoblue">diff</a></li>
          <li><a href="/comparison/a7c1559b7bba/foo?style=monoblue">comparison</a></li>
          <li><a href="/rss-log/tip/foo">rss</a></li>
  <a href="/rev/a7c1559b7bba?style=monoblue">
  <a href="/file/a7c1559b7bba/foo?style=monoblue">file</a>&nbsp;|&nbsp;<a href="/diff/a7c1559b7bba/foo?style=monoblue">diff</a>&nbsp;|&nbsp;<a href="/annotate/a7c1559b7bba/foo?style=monoblue">annotate</a>
  <a href="/rev/43c799df6e75?style=monoblue">
  <a href="/file/43c799df6e75/foo?style=monoblue">file</a>&nbsp;|&nbsp;<a href="/diff/43c799df6e75/foo?style=monoblue">diff</a>&nbsp;|&nbsp;<a href="/annotate/43c799df6e75/foo?style=monoblue">annotate</a>
      <a href="/log/43c799df6e75/foo?style=monoblue">(0)</a><a href="/log/tip/foo?style=monoblue">tip</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'annotate/xyzzy/foo?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/a7c1559b7bba?style=monoblue">graph</a></li>
              <li><a href="/file/a7c1559b7bba/?style=monoblue">files</a></li>
          <li><a href="/file/a7c1559b7bba/foo?style=monoblue">file</a></li>
          <li><a href="/log/a7c1559b7bba/foo?style=monoblue">revisions</a></li>
          <li><a href="/diff/a7c1559b7bba/foo?style=monoblue">diff</a></li>
          <li><a href="/comparison/a7c1559b7bba/foo?style=monoblue">comparison</a></li>
          <li><a href="/raw-annotate/a7c1559b7bba/foo">raw</a></li>
          <dd><a href="/rev/a7c1559b7bba?style=monoblue">a7c1559b7bba</a></dd>
  <a href="/annotate/43c799df6e75/foo?style=monoblue">
  <a href="/annotate/9d8c40cba617/foo?style=monoblue">9d8c40cba617</a>
  <a href="/annotate/43c799df6e75/foo?style=monoblue#l1"
  <a href="/annotate/a7c1559b7bba/foo?style=monoblue#l2"

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'diff/xyzzy/foo?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/a7c1559b7bba?style=monoblue">graph</a></li>
              <li><a href="/file/a7c1559b7bba?style=monoblue">files</a></li>
          <li><a href="/file/a7c1559b7bba/foo?style=monoblue">file</a></li>
          <li><a href="/log/a7c1559b7bba/foo?style=monoblue">revisions</a></li>
          <li><a href="/annotate/a7c1559b7bba/foo?style=monoblue">annotate</a></li>
          <li><a href="/comparison/a7c1559b7bba/foo?style=monoblue">comparison</a></li>
          <li><a href="/raw-diff/a7c1559b7bba/foo">raw</a></li>
          <dd><a href="/rev/a7c1559b7bba?style=monoblue">a7c1559b7bba</a></dd>
  <dd><a href="/diff/43c799df6e75/foo?style=monoblue">43c799df6e75</a></dd>
  <dd><a href="/diff/9d8c40cba617/foo?style=monoblue">9d8c40cba617</a></dd>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'comparison/xyzzy/foo?style=monoblue' | egrep $REVLINKS
              <li><a href="/graph/a7c1559b7bba?style=monoblue">graph</a></li>
              <li><a href="/file/a7c1559b7bba?style=monoblue">files</a></li>
          <li><a href="/file/a7c1559b7bba/foo?style=monoblue">file</a></li>
          <li><a href="/log/a7c1559b7bba/foo?style=monoblue">revisions</a></li>
          <li><a href="/annotate/a7c1559b7bba/foo?style=monoblue">annotate</a></li>
          <li><a href="/diff/a7c1559b7bba/foo?style=monoblue">diff</a></li>
          <li><a href="/raw-diff/a7c1559b7bba/foo">raw</a></li>
          <dd><a href="/rev/a7c1559b7bba?style=monoblue">a7c1559b7bba</a></dd>
  <dd><a href="/comparison/43c799df6e75/foo?style=monoblue">43c799df6e75</a></dd>
  <dd><a href="/comparison/9d8c40cba617/foo?style=monoblue">9d8c40cba617</a></dd>

(De)referencing symbolic revisions (spartan)

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog?style=spartan' | egrep $REVLINKS
  <a href="/log/2?style=spartan">changelog</a>
  <a href="/file/9d8c40cba617/?style=spartan">files</a>
  <a href="/archive/tip.zip">zip</a> 
  navigate: <small class="navigate"><a href="/shortlog/43c799df6e75?style=spartan">(0)</a> <a href="/shortlog/tip?style=spartan">tip</a> </small>
    <td class="node"><a href="/rev/9d8c40cba617?style=spartan">third</a></td>
    <td class="node"><a href="/rev/a7c1559b7bba?style=spartan">second</a></td>
    <td class="node"><a href="/rev/43c799df6e75?style=spartan">first</a></td>
  navigate: <small class="navigate"><a href="/shortlog/43c799df6e75?style=spartan">(0)</a> <a href="/shortlog/tip?style=spartan">tip</a> </small>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'log?style=spartan' | egrep $REVLINKS
  <a href="/shortlog/2?style=spartan">shortlog</a>
  <a href="/file/9d8c40cba617?style=spartan">files</a>
  <a href="/archive/tip.zip">zip</a> 
  navigate: <small class="navigate"><a href="/log/43c799df6e75?style=spartan">(0)</a>  <a href="/log/tip?style=spartan">tip</a> </small>
    <td class="node"><a href="/rev/9d8c40cba617?style=spartan">9d8c40cba617</a></td>
    <th class="files"><a href="/file/9d8c40cba617?style=spartan">files</a>:</th>
    <td class="files"><a href="/diff/9d8c40cba617/foo?style=spartan">foo</a> </td>
    <td class="node"><a href="/rev/a7c1559b7bba?style=spartan">a7c1559b7bba</a></td>
    <th class="files"><a href="/file/a7c1559b7bba?style=spartan">files</a>:</th>
    <td class="files"><a href="/diff/a7c1559b7bba/foo?style=spartan">foo</a> </td>
    <td class="node"><a href="/rev/43c799df6e75?style=spartan">43c799df6e75</a></td>
    <th class="files"><a href="/file/43c799df6e75?style=spartan">files</a>:</th>
    <td class="files"><a href="/diff/43c799df6e75/dir/bar?style=spartan">dir/bar</a> <a href="/diff/43c799df6e75/foo?style=spartan">foo</a> </td>
  navigate: <small class="navigate"><a href="/log/43c799df6e75?style=spartan">(0)</a>  <a href="/log/tip?style=spartan">tip</a> </small>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'graph?style=spartan' | egrep $REVLINKS
  <a href="/file/9d8c40cba617/?style=spartan">files</a>
  navigate: <small class="navigate"><a href="/graph/43c799df6e75?style=spartan">(0)</a> <a href="/graph/tip?style=spartan">tip</a> </small>
  navigate: <small class="navigate"><a href="/graph/43c799df6e75?style=spartan">(0)</a> <a href="/graph/tip?style=spartan">tip</a> </small>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'tags?style=spartan' | egrep $REVLINKS
  <a href="/rev/9d8c40cba617?style=spartan">tip</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'branches?style=spartan' | egrep $REVLINKS
  <a href="/shortlog/9d8c40cba617?style=spartan" class="open">default</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file?style=spartan' | egrep $REVLINKS
  <a href="/log/2?style=spartan">changelog</a>
  <a href="/shortlog/2?style=spartan">shortlog</a>
  <a href="/rev/9d8c40cba617?style=spartan">changeset</a>
  <a href="/archive/9d8c40cba617.zip">zip</a> 
  <h2><a href="/">Mercurial</a>  / files for changeset <a href="/rev/9d8c40cba617">9d8c40cba617</a>: /</h2>
    <td><a href="/file/9d8c40cba617/?style=spartan">[up]</a>
  <a href="/file/9d8c40cba617/dir?style=spartan">dir/</a>
  <a href="/file/9d8c40cba617/dir/?style=spartan">
  <td><a href="/file/9d8c40cba617/foo?style=spartan">foo</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog?style=spartan&rev=all()' | egrep $REVLINKS
  <a href="/archive/tip.zip">zip</a> 
    <td class="node"><a href="/rev/9d8c40cba617?style=spartan">9d8c40cba617</a></td>
  <a href="/rev/a7c1559b7bba?style=spartan">a7c1559b7bba</a>
    <th class="files"><a href="/file/9d8c40cba617?style=spartan">files</a>:</th>
    <td class="files"><a href="/diff/9d8c40cba617/foo?style=spartan">foo</a> </td>
    <td class="node"><a href="/rev/a7c1559b7bba?style=spartan">a7c1559b7bba</a></td>
  <a href="/rev/43c799df6e75?style=spartan">43c799df6e75</a>
  <td class="child"><a href="/rev/9d8c40cba617?style=spartan">9d8c40cba617</a></td>
    <th class="files"><a href="/file/a7c1559b7bba?style=spartan">files</a>:</th>
    <td class="files"><a href="/diff/a7c1559b7bba/foo?style=spartan">foo</a> </td>
    <td class="node"><a href="/rev/43c799df6e75?style=spartan">43c799df6e75</a></td>
  <td class="child"><a href="/rev/a7c1559b7bba?style=spartan">a7c1559b7bba</a></td>
    <th class="files"><a href="/file/43c799df6e75?style=spartan">files</a>:</th>
    <td class="files"><a href="/diff/43c799df6e75/dir/bar?style=spartan">dir/bar</a> <a href="/diff/43c799df6e75/foo?style=spartan">foo</a> </td>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'rev/xyzzy?style=spartan' | egrep $REVLINKS
  <a href="/log/1?style=spartan">changelog</a>
  <a href="/shortlog/1?style=spartan">shortlog</a>
  <a href="/file/a7c1559b7bba?style=spartan">files</a>
  <a href="/raw-rev/a7c1559b7bba">raw</a>
  <a href="/archive/a7c1559b7bba.zip">zip</a> 
   <td class="changeset"><a href="/rev/a7c1559b7bba?style=spartan">a7c1559b7bba</a></td>
  <td class="parent"><a href="/rev/43c799df6e75?style=spartan">43c799df6e75</a></td>
  <td class="child"><a href="/rev/9d8c40cba617?style=spartan">9d8c40cba617</a></td>
   <td class="files"><a href="/file/a7c1559b7bba/foo?style=spartan">foo</a> </td>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'shortlog/xyzzy?style=spartan' | egrep $REVLINKS
  <a href="/log/1?style=spartan">changelog</a>
  <a href="/file/a7c1559b7bba/?style=spartan">files</a>
  <a href="/archive/tip.zip">zip</a> 
  navigate: <small class="navigate"><a href="/shortlog/43c799df6e75?style=spartan">(0)</a> <a href="/shortlog/tip?style=spartan">tip</a> </small>
    <td class="node"><a href="/rev/a7c1559b7bba?style=spartan">second</a></td>
    <td class="node"><a href="/rev/43c799df6e75?style=spartan">first</a></td>
  navigate: <small class="navigate"><a href="/shortlog/43c799df6e75?style=spartan">(0)</a> <a href="/shortlog/tip?style=spartan">tip</a> </small>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'log/xyzzy?style=spartan' | egrep $REVLINKS
  <a href="/shortlog/1?style=spartan">shortlog</a>
  <a href="/file/a7c1559b7bba?style=spartan">files</a>
  <a href="/archive/tip.zip">zip</a> 
  navigate: <small class="navigate"><a href="/log/43c799df6e75?style=spartan">(0)</a>  <a href="/log/tip?style=spartan">tip</a> </small>
    <td class="node"><a href="/rev/a7c1559b7bba?style=spartan">a7c1559b7bba</a></td>
    <th class="files"><a href="/file/a7c1559b7bba?style=spartan">files</a>:</th>
    <td class="files"><a href="/diff/a7c1559b7bba/foo?style=spartan">foo</a> </td>
    <td class="node"><a href="/rev/43c799df6e75?style=spartan">43c799df6e75</a></td>
    <th class="files"><a href="/file/43c799df6e75?style=spartan">files</a>:</th>
    <td class="files"><a href="/diff/43c799df6e75/dir/bar?style=spartan">dir/bar</a> <a href="/diff/43c799df6e75/foo?style=spartan">foo</a> </td>
  navigate: <small class="navigate"><a href="/log/43c799df6e75?style=spartan">(0)</a>  <a href="/log/tip?style=spartan">tip</a> </small>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'graph/xyzzy?style=spartan' | egrep $REVLINKS
  <a href="/file/a7c1559b7bba/?style=spartan">files</a>
  navigate: <small class="navigate"><a href="/graph/43c799df6e75?style=spartan">(0)</a> <a href="/graph/tip?style=spartan">tip</a> </small>
  navigate: <small class="navigate"><a href="/graph/43c799df6e75?style=spartan">(0)</a> <a href="/graph/tip?style=spartan">tip</a> </small>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/xyzzy?style=spartan' | egrep $REVLINKS
  <a href="/log/1?style=spartan">changelog</a>
  <a href="/shortlog/1?style=spartan">shortlog</a>
  <a href="/rev/a7c1559b7bba?style=spartan">changeset</a>
  <a href="/archive/a7c1559b7bba.zip">zip</a> 
  <h2><a href="/">Mercurial</a>  / files for changeset <a href="/rev/a7c1559b7bba">a7c1559b7bba</a>: /</h2>
    <td><a href="/file/a7c1559b7bba/?style=spartan">[up]</a>
  <a href="/file/a7c1559b7bba/dir?style=spartan">dir/</a>
  <a href="/file/a7c1559b7bba/dir/?style=spartan">
  <td><a href="/file/a7c1559b7bba/foo?style=spartan">foo</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/xyzzy/foo?style=spartan' | egrep $REVLINKS
  <a href="/log/1?style=spartan">changelog</a>
  <a href="/shortlog/1?style=spartan">shortlog</a>
  <a href="/rev/a7c1559b7bba?style=spartan">changeset</a>
  <a href="/file/a7c1559b7bba/?style=spartan">files</a>
  <a href="/log/a7c1559b7bba/foo?style=spartan">revisions</a>
  <a href="/annotate/a7c1559b7bba/foo?style=spartan">annotate</a>
  <a href="/raw-file/a7c1559b7bba/foo">raw</a>
   <td><a href="/rev/a7c1559b7bba?style=spartan">a7c1559b7bba</a></td>
  <a href="/file/43c799df6e75/foo?style=spartan">
  <td><a href="/file/9d8c40cba617/foo?style=spartan">9d8c40cba617</a></td>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'log/xyzzy/foo?style=spartan' | egrep $REVLINKS
     href="/atom-log/tip/foo" title="Atom feed for test:foo">
     href="/rss-log/tip/foo" title="RSS feed for test:foo">
  <a href="/file/a7c1559b7bba/foo?style=spartan">file</a>
  <a href="/annotate/a7c1559b7bba/foo?style=spartan">annotate</a>
  <a type="application/rss+xml" href="/rss-log/tip/foo">rss</a>
  <a type="application/atom+xml" href="/atom-log/tip/foo" title="Atom feed for test:foo">atom</a>
  <p>navigate: <small class="navigate"><a href="/log/43c799df6e75/foo?style=spartan">(0)</a> <a href="/log/tip/foo?style=spartan">tip</a> </small></p>
    <th class="firstline"><a href="/rev/a7c1559b7bba?style=spartan">second</a></th>
     <a href="/file/a7c1559b7bba/foo?style=spartan">a7c1559b7bba</a>
     <a href="/diff/a7c1559b7bba/foo?style=spartan">(diff)</a>
     <a href="/annotate/a7c1559b7bba/foo?style=spartan">(annotate)</a>
    <th class="firstline"><a href="/rev/43c799df6e75?style=spartan">first</a></th>
     <a href="/file/43c799df6e75/foo?style=spartan">43c799df6e75</a>
     <a href="/diff/43c799df6e75/foo?style=spartan">(diff)</a>
     <a href="/annotate/43c799df6e75/foo?style=spartan">(annotate)</a>

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'annotate/xyzzy/foo?style=spartan' | egrep $REVLINKS
  <a href="/log/1?style=spartan">changelog</a>
  <a href="/shortlog/1?style=spartan">shortlog</a>
  <a href="/rev/a7c1559b7bba?style=spartan">changeset</a>
  <a href="/file/a7c1559b7bba/?style=spartan">files</a>
  <a href="/file/a7c1559b7bba/foo?style=spartan">file</a>
  <a href="/log/a7c1559b7bba/foo?style=spartan">revisions</a>
  <a href="/raw-annotate/a7c1559b7bba/foo">raw</a>
   <td><a href="/rev/a7c1559b7bba?style=spartan">a7c1559b7bba</a></td>
  <a href="/annotate/43c799df6e75/foo?style=spartan">
  <td><a href="/annotate/9d8c40cba617/foo?style=spartan">9d8c40cba617</a></td>
  <a href="/annotate/43c799df6e75/foo?style=spartan#l1"
  <a href="/annotate/a7c1559b7bba/foo?style=spartan#l2"

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'diff/xyzzy/foo?style=spartan' | egrep $REVLINKS
  <a href="/log/1?style=spartan">changelog</a>
  <a href="/shortlog/1?style=spartan">shortlog</a>
  <a href="/rev/a7c1559b7bba?style=spartan">changeset</a>
  <a href="/file/a7c1559b7bba/foo?style=spartan">file</a>
  <a href="/log/a7c1559b7bba/foo?style=spartan">revisions</a>
  <a href="/annotate/a7c1559b7bba/foo?style=spartan">annotate</a>
  <a href="/raw-diff/a7c1559b7bba/foo">raw</a>
   <td class="revision"><a href="/rev/a7c1559b7bba?style=spartan">a7c1559b7bba</a></td>
  <td class="parent"><a href="/rev/43c799df6e75?style=spartan">43c799df6e75</a></td>
  <td class="child"><a href="/rev/9d8c40cba617?style=spartan">9d8c40cba617</a></td>

Done

  $ cat errors.log
  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS
  $ cd ..
