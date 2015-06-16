#require serve

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
  >>> open('msg', 'wb').write('branch commit with null character: \0\n')
  $ hg ci -l msg
  $ rm msg

  $ cat > .hg/hgrc <<EOF
  > [graph]
  > default.width = 3
  > stable.width = 3
  > stable.color = FF0000
  > [websub]
  > append = s|(.*)|\1(websub)|
  > EOF

  $ hg serve --config server.uncompressed=False -n test -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg log -G --template '{rev}:{node|short} {desc}\n'
  @  3:cad8025a2e87 branch commit with null character: \x00 (esc)
  |
  o  2:1d22e65f027e branch
  |
  o  1:a4f92ed23982 Added tag 1.0 for changeset 2ef0ac749a14
  |
  o  0:2ef0ac749a14 base
  

Logs and changes

  $ get-with-headers.py 127.0.0.1:$HGPORT 'log/?style=atom'
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
    <title>[unstable] branch commit with null character: </title>
    <id>http://*:$HGPORT/#changeset-cad8025a2e87f88c06259790adfa15acb4080123</id> (glob)
    <link href="http://*:$HGPORT/rev/cad8025a2e87"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
  	<table xmlns="http://www.w3.org/1999/xhtml">
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>cad8025a2e87</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td>unstable</td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td>something</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td>tip</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>branch commit with null character: (websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td></td>
  	</tr>
  	</table>
    </content>
   </entry>
   <entry>
    <title>[stable] branch</title>
    <id>http://*:$HGPORT/#changeset-1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe</id> (glob)
    <link href="http://*:$HGPORT/rev/1d22e65f027e"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
  	<table xmlns="http://www.w3.org/1999/xhtml">
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>1d22e65f027e</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td>stable</td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>branch(websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td>foo<br /></td>
  	</tr>
  	</table>
    </content>
   </entry>
   <entry>
    <title>[default] Added tag 1.0 for changeset 2ef0ac749a14</title>
    <id>http://*:$HGPORT/#changeset-a4f92ed23982be056b9852de5dfe873eaac7f0de</id> (glob)
    <link href="http://*:$HGPORT/rev/a4f92ed23982"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
  	<table xmlns="http://www.w3.org/1999/xhtml">
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>a4f92ed23982</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td>default</td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>Added tag 1.0 for changeset 2ef0ac749a14(websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td>.hgtags<br /></td>
  	</tr>
  	</table>
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
  	<table xmlns="http://www.w3.org/1999/xhtml">
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>2ef0ac749a14</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td></td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td>anotherthing</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td>1.0</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>base(websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td>da/foo<br />foo<br /></td>
  	</tr>
  	</table>
    </content>
   </entry>
  
  </feed>
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log/?style=rss'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <rss version="2.0">
    <channel>
      <link>http://*:$HGPORT/</link> (glob)
      <language>en-us</language>
  
      <title>test Changelog</title>
      <description>test Changelog</description>
      <item>
      <title>[unstable] branch commit with null character: </title>
      <guid isPermaLink="true">http://*:$HGPORT/rev/cad8025a2e87</guid> (glob)
               <link>http://*:$HGPORT/rev/cad8025a2e87</link> (glob)
      <description>
                <![CDATA[
  	<table>
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>cad8025a2e87</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td>unstable</td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td>something</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td>tip</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>branch commit with null character: (websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td></td>
  	</tr>
  	</table>
  	]]></description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  <item>
      <title>[stable] branch</title>
      <guid isPermaLink="true">http://*:$HGPORT/rev/1d22e65f027e</guid> (glob)
               <link>http://*:$HGPORT/rev/1d22e65f027e</link> (glob)
      <description>
                <![CDATA[
  	<table>
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>1d22e65f027e</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td>stable</td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>branch(websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td>foo<br /></td>
  	</tr>
  	</table>
  	]]></description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  <item>
      <title>[default] Added tag 1.0 for changeset 2ef0ac749a14</title>
      <guid isPermaLink="true">http://*:$HGPORT/rev/a4f92ed23982</guid> (glob)
               <link>http://*:$HGPORT/rev/a4f92ed23982</link> (glob)
      <description>
                <![CDATA[
  	<table>
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>a4f92ed23982</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td>default</td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>Added tag 1.0 for changeset 2ef0ac749a14(websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td>.hgtags<br /></td>
  	</tr>
  	</table>
  	]]></description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  <item>
      <title>base</title>
      <guid isPermaLink="true">http://*:$HGPORT/rev/2ef0ac749a14</guid> (glob)
               <link>http://*:$HGPORT/rev/2ef0ac749a14</link> (glob)
      <description>
                <![CDATA[
  	<table>
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>2ef0ac749a14</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td></td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td>anotherthing</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td>1.0</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>base(websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td>da/foo<br />foo<br /></td>
  	</tr>
  	</table>
  	]]></description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  
    </channel>
  </rss> (no-eol)
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log/1/?style=atom'
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
    <title>[default] Added tag 1.0 for changeset 2ef0ac749a14</title>
    <id>http://*:$HGPORT/#changeset-a4f92ed23982be056b9852de5dfe873eaac7f0de</id> (glob)
    <link href="http://*:$HGPORT/rev/a4f92ed23982"/> (glob)
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
  	<table xmlns="http://www.w3.org/1999/xhtml">
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>a4f92ed23982</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td>default</td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>Added tag 1.0 for changeset 2ef0ac749a14(websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td>.hgtags<br /></td>
  	</tr>
  	</table>
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
  	<table xmlns="http://www.w3.org/1999/xhtml">
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>2ef0ac749a14</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td></td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td>anotherthing</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td>1.0</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>base(websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td>da/foo<br />foo<br /></td>
  	</tr>
  	</table>
    </content>
   </entry>
  
  </feed>
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log/1/?style=rss'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <rss version="2.0">
    <channel>
      <link>http://*:$HGPORT/</link> (glob)
      <language>en-us</language>
  
      <title>test Changelog</title>
      <description>test Changelog</description>
      <item>
      <title>[default] Added tag 1.0 for changeset 2ef0ac749a14</title>
      <guid isPermaLink="true">http://*:$HGPORT/rev/a4f92ed23982</guid> (glob)
               <link>http://*:$HGPORT/rev/a4f92ed23982</link> (glob)
      <description>
                <![CDATA[
  	<table>
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>a4f92ed23982</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td>default</td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>Added tag 1.0 for changeset 2ef0ac749a14(websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td>.hgtags<br /></td>
  	</tr>
  	</table>
  	]]></description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  <item>
      <title>base</title>
      <guid isPermaLink="true">http://*:$HGPORT/rev/2ef0ac749a14</guid> (glob)
               <link>http://*:$HGPORT/rev/2ef0ac749a14</link> (glob)
      <description>
                <![CDATA[
  	<table>
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>2ef0ac749a14</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td></td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td>anotherthing</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td>1.0</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>base(websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td>da/foo<br />foo<br /></td>
  	</tr>
  	</table>
  	]]></description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  
    </channel>
  </rss> (no-eol)
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log/1/foo/?style=atom'
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
  	<table xmlns="http://www.w3.org/1999/xhtml">
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>2ef0ac749a14</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td></td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td>anotherthing</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td>1.0</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>base(websub)</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td></td>
  	</tr>
  	</table>
    </content>
   </entry>
  
  </feed>
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log/1/foo/?style=rss'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <rss version="2.0">
    <channel>
      <link>http://*:$HGPORT/</link> (glob)
      <language>en-us</language>
  
      <title>test: foo history</title>
      <description>foo revision history</description>
      <item>
      <title>base</title>
      <link>http://*:$HGPORT/log2ef0ac749a14/foo</link> (glob)
      <description><![CDATA[base(websub)]]></description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  
    </channel>
  </rss>
  $ get-with-headers.py 127.0.0.1:$HGPORT 'shortlog/'
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
  
  </ul>
  <ul>
   <li><a href="/help">help</a></li>
  </ul>
  <div class="atom-logo">
  <a href="/atom-log" title="subscribe to atom feed">
  <img class="atom-logo" src="/static/feed-icon-14x14.png" alt="atom feed" />
  </a>
  </div>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>log</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" value="" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <div class="navigate">
  <a href="/shortlog/tip?revcount=30">less</a>
  <a href="/shortlog/tip?revcount=120">more</a>
  | rev 3: <a href="/shortlog/2ef0ac749a14">(0)</a> <a href="/shortlog/tip">tip</a> 
  </div>
  
  <table class="bigtable">
  <thead>
   <tr>
    <th class="age">age</th>
    <th class="author">author</th>
    <th class="description">description</th>
   </tr>
  </thead>
  <tbody class="stripes2">
   <tr>
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description">
     <a href="/rev/cad8025a2e87">branch commit with null character: </a>
     <span class="branchhead">unstable</span> <span class="tag">tip</span> <span class="tag">something</span> 
    </td>
   </tr>
   <tr>
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description">
     <a href="/rev/1d22e65f027e">branch</a>
     <span class="branchhead">stable</span> 
    </td>
   </tr>
   <tr>
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description">
     <a href="/rev/a4f92ed23982">Added tag 1.0 for changeset 2ef0ac749a14</a>
     <span class="branchhead">default</span> 
    </td>
   </tr>
   <tr>
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description">
     <a href="/rev/2ef0ac749a14">base</a>
     <span class="tag">1.0</span> <span class="tag">anotherthing</span> 
    </td>
   </tr>
  
  </tbody>
  </table>
  
  <div class="navigate">
  <a href="/shortlog/tip?revcount=30">less</a>
  <a href="/shortlog/tip?revcount=120">more</a>
  | rev 3: <a href="/shortlog/2ef0ac749a14">(0)</a> <a href="/shortlog/tip">tip</a> 
  </div>
  
  <script type="text/javascript">
      ajaxScrollInit(
              '/shortlog/%next%',
              '', <!-- NEXTHASH
              function (htmlText, previousVal) {
                  var m = htmlText.match(/'(\w+)', <!-- NEXTHASH/);
                  return m ? m[1] : null;
              },
              '.bigtable > tbody',
              '<tr class="%class%">\
              <td colspan="3" style="text-align: center;">%text%</td>\
              </tr>'
      );
  </script>
  
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'rev/0/'
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
  <h3>changeset 0:2ef0ac749a14  <span class="tag">1.0</span>  <span class="tag">anotherthing</span> </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <div class="description">base(websub)</div>
  
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
  
      <a id="diffstatexpand" href="javascript:toggleDiffstat()">[<tt>+</tt>]</a>
      <div id="diffstatdetails" style="display:none;">
        <a href="javascript:toggleDiffstat()">[<tt>-</tt>]</a>
        <table class="diffstat-table stripes2">  <tr>
      <td class="diffstat-file"><a href="#l1.1">da/foo</a></td>
      <td class="diffstat-total" align="right">1</td>
      <td class="diffstat-graph">
        <span class="diffstat-add" style="width:100.0%;">&nbsp;</span>
        <span class="diffstat-remove" style="width:0.0%;">&nbsp;</span>
      </td>
    </tr>
    <tr>
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
  <div class="sourcefirst linewraptoggle">line wrap: <a class="linewraplink" href="javascript:toggleLinewrap()">on</a></div>
  <div class="sourcefirst"> line diff</div>
  <div class="stripes2 diffblocks">
  <div class="bottomline inc-lineno"><pre class="sourcelines wrap">
  <span id="l1.1" class="minusline">--- /dev/null	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l1.1"></a>
  <span id="l1.2" class="plusline">+++ b/da/foo	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l1.2"></a>
  <span id="l1.3" class="atline">@@ -0,0 +1,1 @@</span><a href="#l1.3"></a>
  <span id="l1.4" class="plusline">+foo</span><a href="#l1.4"></a></pre></div><div class="bottomline inc-lineno"><pre class="sourcelines wrap">
  <span id="l2.1" class="minusline">--- /dev/null	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l2.1"></a>
  <span id="l2.2" class="plusline">+++ b/foo	Thu Jan 01 00:00:00 1970 +0000</span><a href="#l2.2"></a>
  <span id="l2.3" class="atline">@@ -0,0 +1,1 @@</span><a href="#l2.3"></a>
  <span id="l2.4" class="plusline">+foo</span><a href="#l2.4"></a></pre></div>
  </div>
  </div>
  
  </div>
  </div>
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'rev/1/?style=raw'
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
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log?rev=base'
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
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>searching for 'base'</h3>
  
  <p>
  Assuming literal keyword search.
  
  
  </p>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" value="base"></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <div class="navigate">
  <a href="/log?rev=base&revcount=5">less</a>
  <a href="/log?rev=base&revcount=20">more</a>
  </div>
  
  <table class="bigtable">
  <thead>
   <tr>
    <th class="age">age</th>
    <th class="author">author</th>
    <th class="description">description</th>
   </tr>
  </thead>
  <tbody class="stripes2">
   <tr>
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description">
     <a href="/rev/2ef0ac749a14">base</a>
     <span class="tag">1.0</span> <span class="tag">anotherthing</span> 
    </td>
   </tr>
  
  </tbody>
  </table>
  
  <div class="navigate">
  <a href="/log?rev=base&revcount=5">less</a>
  <a href="/log?rev=base&revcount=20">more</a>
  </div>
  
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log?rev=stable&style=raw' | grep 'revision:'
  revision:    2

Search with revset syntax

  $ get-with-headers.py 127.0.0.1:$HGPORT 'log?rev=tip^&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID cad8025a2e87f88c06259790adfa15acb4080123
  # Query "tip^"
  # Mode revset expression search
  
  changeset:   1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe
  revision:    2
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     branch
  branch:      stable
  
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log?rev=last(all(),2)^&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID cad8025a2e87f88c06259790adfa15acb4080123
  # Query "last(all(),2)^"
  # Mode revset expression search
  
  changeset:   1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe
  revision:    2
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     branch
  branch:      stable
  
  changeset:   a4f92ed23982be056b9852de5dfe873eaac7f0de
  revision:    1
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     Added tag 1.0 for changeset 2ef0ac749a14
  branch:      default
  
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log?rev=last(all(,2)^&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID cad8025a2e87f88c06259790adfa15acb4080123
  # Query "last(all(,2)^"
  # Mode literal keyword search
  
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log?rev=last(al(),2)^&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID cad8025a2e87f88c06259790adfa15acb4080123
  # Query "last(al(),2)^"
  # Mode literal keyword search
  
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log?rev=bookmark(anotherthing)&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID cad8025a2e87f88c06259790adfa15acb4080123
  # Query "bookmark(anotherthing)"
  # Mode revset expression search
  
  changeset:   2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  revision:    0
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     base
  tag:         1.0
  bookmark:    anotherthing
  
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log?rev=bookmark(abc)&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID cad8025a2e87f88c06259790adfa15acb4080123
  # Query "bookmark(abc)"
  # Mode literal keyword search
  
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log?rev=deadbeef:&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID cad8025a2e87f88c06259790adfa15acb4080123
  # Query "deadbeef:"
  # Mode literal keyword search
  
  

  $ get-with-headers.py 127.0.0.1:$HGPORT 'log?rev=user("test")&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID cad8025a2e87f88c06259790adfa15acb4080123
  # Query "user("test")"
  # Mode revset expression search
  
  changeset:   cad8025a2e87f88c06259790adfa15acb4080123
  revision:    3
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     branch commit with null character: \x00 (esc)
  branch:      unstable
  tag:         tip
  bookmark:    something
  
  changeset:   1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe
  revision:    2
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     branch
  branch:      stable
  
  changeset:   a4f92ed23982be056b9852de5dfe873eaac7f0de
  revision:    1
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     Added tag 1.0 for changeset 2ef0ac749a14
  branch:      default
  
  changeset:   2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  revision:    0
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     base
  tag:         1.0
  bookmark:    anotherthing
  
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'log?rev=user("re:test")&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID cad8025a2e87f88c06259790adfa15acb4080123
  # Query "user("re:test")"
  # Mode literal keyword search
  
  

File-related

  $ get-with-headers.py 127.0.0.1:$HGPORT 'file/1/foo/?style=raw'
  200 Script output follows
  
  foo
  $ get-with-headers.py 127.0.0.1:$HGPORT 'annotate/1/foo/?style=raw'
  200 Script output follows
  
  
  test@0: foo
  
  
  
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'file/1/?style=raw'
  200 Script output follows
  
  
  drwxr-xr-x da
  -rw-r--r-- 45 .hgtags
  -rw-r--r-- 4 foo
  
  
  $ hg log --template "{file_mods}\n" -r 1
  
  $ hg parents --template "{node|short}\n" -r 1
  2ef0ac749a14
  $ hg parents --template "{node|short}\n" -r 1 foo
  2ef0ac749a14

  $ get-with-headers.py 127.0.0.1:$HGPORT 'file/1/foo'
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
  <li><a href="/shortlog/1">log</a></li>
  <li><a href="/graph/1">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/1">changeset</a></li>
  <li><a href="/file/1/">browse</a></li>
  </ul>
  <ul>
  <li class="active">file</li>
  <li><a href="/file/tip/foo">latest</a></li>
  <li><a href="/diff/1/foo">diff</a></li>
  <li><a href="/comparison/1/foo">comparison</a></li>
  <li><a href="/annotate/1/foo">annotate</a></li>
  <li><a href="/log/1/foo">file log</a></li>
  <li><a href="/raw-file/1/foo">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>view foo @ 1:a4f92ed23982 </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <div class="description">Added tag 1.0 for changeset 2ef0ac749a14(websub)</div>
  
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
   <td class="author"><a href="/file/2ef0ac749a14/foo">2ef0ac749a14</a> </td>
  </tr>
  <tr>
   <th class="author">children</th>
   <td class="author"><a href="/file/1d22e65f027e/foo">1d22e65f027e</a> </td>
  </tr>
  </table>
  
  <div class="overflow">
  <div class="sourcefirst linewraptoggle">line wrap: <a class="linewraplink" href="javascript:toggleLinewrap()">on</a></div>
  <div class="sourcefirst"> line source</div>
  <pre class="sourcelines stripes4 wrap">
  <span id="l1">foo</span><a href="#l1"></a></pre>
  <div class="sourcelast"></div>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'filediff/0/foo/?style=raw'
  200 Script output follows
  
  
  diff -r 000000000000 -r 2ef0ac749a14 foo
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +foo
  
  
  
  

  $ get-with-headers.py 127.0.0.1:$HGPORT 'filediff/1/foo/?style=raw'
  200 Script output follows
  
  
  
  
  
  

  $ hg log --template "{file_mods}\n" -r 2
  foo
  $ hg parents --template "{node|short}\n" -r 2
  a4f92ed23982
  $ hg parents --template "{node|short}\n" -r 2 foo
  2ef0ac749a14

  $ get-with-headers.py 127.0.0.1:$HGPORT 'file/2/foo'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: 1d22e65f027e foo</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog/2">log</a></li>
  <li><a href="/graph/2">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li><a href="/rev/2">changeset</a></li>
  <li><a href="/file/2/">browse</a></li>
  </ul>
  <ul>
  <li class="active">file</li>
  <li><a href="/file/tip/foo">latest</a></li>
  <li><a href="/diff/2/foo">diff</a></li>
  <li><a href="/comparison/2/foo">comparison</a></li>
  <li><a href="/annotate/2/foo">annotate</a></li>
  <li><a href="/log/2/foo">file log</a></li>
  <li><a href="/raw-file/2/foo">raw</a></li>
  </ul>
  <ul>
  <li><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>view foo @ 2:1d22e65f027e <span class="branchname">stable</span> </h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  
  <div class="description">branch(websub)</div>
  
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
   <td class="author"><a href="/file/2ef0ac749a14/foo">2ef0ac749a14</a> </td>
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
  <span id="l1">another</span><a href="#l1"></a></pre>
  <div class="sourcelast"></div>
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  


Overviews

  $ get-with-headers.py 127.0.0.1:$HGPORT 'raw-tags'
  200 Script output follows
  
  tip	cad8025a2e87f88c06259790adfa15acb4080123
  1.0	2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  $ get-with-headers.py 127.0.0.1:$HGPORT 'raw-branches'
  200 Script output follows
  
  unstable	cad8025a2e87f88c06259790adfa15acb4080123	open
  stable	1d22e65f027e5a0609357e7d8e7508cd2ba5d2fe	inactive
  default	a4f92ed23982be056b9852de5dfe873eaac7f0de	inactive
  $ get-with-headers.py 127.0.0.1:$HGPORT 'raw-bookmarks'
  200 Script output follows
  
  anotherthing	2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  something	cad8025a2e87f88c06259790adfa15acb4080123
  $ get-with-headers.py 127.0.0.1:$HGPORT 'summary/?style=gitweb'
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
  <a href="http://mercurial.selenic.com/" title="Mercurial" style="float: right;">Mercurial</a>
  <a href="/">Mercurial</a>  / summary
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
  <a href="/file?style=gitweb">files</a> |
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
  <a class="list" href="/rev/cad8025a2e87?style=gitweb">
  <b>branch commit with null character: </b>
  <span class="logtags"><span class="branchtag" title="unstable">unstable</span> <span class="tagtag" title="tip">tip</span> <span class="bookmarktag" title="something">something</span> </span>
  </a>
  </td>
  <td class="link" nowrap>
  <a href="/rev/cad8025a2e87?style=gitweb">changeset</a> |
  <a href="/file/cad8025a2e87?style=gitweb">files</a>
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
  <td><a class="list" href="/rev/cad8025a2e87?style=gitweb"><b>something</b></a></td>
  <td class="link">
  <a href="/rev/cad8025a2e87?style=gitweb">changeset</a> |
  <a href="/log/cad8025a2e87?style=gitweb">changelog</a> |
  <a href="/file/cad8025a2e87?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="light"><td colspan="3"><a class="list" href="/bookmarks?style=gitweb">...</a></td></tr>
  </table>
  
  <div><a class="title" href="/branches?style=gitweb">branches</a></div>
  <table cellspacing="0">
  
  <tr class="parity0">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><a class="list" href="/shortlog/cad8025a2e87?style=gitweb"><b>cad8025a2e87</b></a></td>
  <td class="">unstable</td>
  <td class="link">
  <a href="/changeset/cad8025a2e87?style=gitweb">changeset</a> |
  <a href="/log/cad8025a2e87?style=gitweb">changelog</a> |
  <a href="/file/cad8025a2e87?style=gitweb">files</a>
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
    <td colspan="4"><a class="list"  href="/branches?style=gitweb">...</a></td>
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
  
  $ get-with-headers.py 127.0.0.1:$HGPORT 'graph/?style=gitweb'
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
  <a href="http://mercurial.selenic.com/" title="Mercurial" style="float: right;">Mercurial</a>
  <a href="/">Mercurial</a>  / graph
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
  <a href="/log/tip?style=gitweb">changelog</a> |
  graph |
  <a href="/tags?style=gitweb">tags</a> |
  <a href="/bookmarks?style=gitweb">bookmarks</a> |
  <a href="/branches?style=gitweb">branches</a> |
  <a href="/file/tip?style=gitweb">files</a> |
  <a href="/help?style=gitweb">help</a>
  <br/>
  <a href="/graph/tip?revcount=30&style=gitweb">less</a>
  <a href="/graph/tip?revcount=120&style=gitweb">more</a>
  | <a href="/graph/2ef0ac749a14?style=gitweb">(0)</a> <a href="/graph/tip?style=gitweb">tip</a> <br/>
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
  
  var data = [["cad8025a2e87", [0, 1], [[0, 0, 1, 3, "FF0000"]], "branch commit with null character: \u0000", "test", "1970-01-01", ["unstable", true], ["tip"], ["something"]], ["1d22e65f027e", [0, 1], [[0, 0, 1, 3, ""]], "branch", "test", "1970-01-01", ["stable", true], [], []], ["a4f92ed23982", [0, 1], [[0, 0, 1, 3, ""]], "Added tag 1.0 for changeset 2ef0ac749a14", "test", "1970-01-01", ["default", true], [], []], ["2ef0ac749a14", [0, 1], [], "base", "test", "1970-01-01", ["default", false], ["1.0"], ["anotherthing"]]];
  var graph = new Graph();
  graph.scale(39);
  
  graph.vertex = function(x, y, color, parity, cur) {
  	
  	this.ctx.beginPath();
  	color = this.setColor(color, 0.25, 0.75);
  	this.ctx.arc(x, y, radius, 0, Math.PI * 2, true);
  	this.ctx.fill();
  	
  	var bg = '<li class="bg parity' + parity + '"></li>';
  	var left = (this.bg_height - this.box_size) + (this.columns + 1) * this.box_size;
  	var nstyle = 'padding-left: ' + left + 'px;';
  	
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
  	
  	var item = '<li style="' + nstyle + '"><span class="desc">';
  	item += '<a class="list" href="/rev/' + cur[0] + '?style=gitweb" title="' + cur[0] + '"><b>' + cur[3] + '</b></a>';
  	item += '</span> ' + tagspan + '';
  	item += '<span class="info">' + cur[5] + ', by ' + cur[4] + '</span></li>';
  
  	return [bg, item];
  	
  }
  
  graph.render(data);
  
  // stop hiding script -->
  </script>
  
  <div class="page_nav">
  <a href="/graph/tip?revcount=30&style=gitweb">less</a>
  <a href="/graph/tip?revcount=120&style=gitweb">more</a>
  | <a href="/graph/2ef0ac749a14?style=gitweb">(0)</a> <a href="/graph/tip?style=gitweb">tip</a> 
  </div>
  
  <script type="text/javascript">
      ajaxScrollInit(
              '/graph/3?revcount=%next%&style=gitweb',
              60+60,
              function (htmlText, previousVal) { return previousVal + 60; },
              '#wrapper',
              '<div class="%class%" style="text-align: center;">%text%</div>',
              'graph'
      );
  </script>
  
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
  
raw graph

  $ get-with-headers.py 127.0.0.1:$HGPORT 'graph/?style=raw'
  200 Script output follows
  
  
  # HG graph
  # Node ID cad8025a2e87f88c06259790adfa15acb4080123
  # Rows shown 4
  
  changeset:   cad8025a2e87
  user:        test
  date:        1970-01-01
  summary:     branch commit with null character: \x00 (esc)
  branch:      unstable
  tag:         tip
  bookmark:    something
  
  node:        (0, 0) (color 1)
  edge:        (0, 0) -> (0, 1) (color 1)
  
  changeset:   1d22e65f027e
  user:        test
  date:        1970-01-01
  summary:     branch
  branch:      stable
  
  node:        (0, 1) (color 1)
  edge:        (0, 1) -> (0, 2) (color 1)
  
  changeset:   a4f92ed23982
  user:        test
  date:        1970-01-01
  summary:     Added tag 1.0 for changeset 2ef0ac749a14
  branch:      default
  
  node:        (0, 2) (color 1)
  edge:        (0, 2) -> (0, 3) (color 1)
  
  changeset:   2ef0ac749a14
  user:        test
  date:        1970-01-01
  summary:     base
  tag:         1.0
  bookmark:    anotherthing
  
  node:        (0, 3) (color 1)
  
  

capabilities

  $ get-with-headers.py 127.0.0.1:$HGPORT '?cmd=capabilities'; echo
  200 Script output follows
  
  lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1*%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 (glob)

heads

  $ get-with-headers.py 127.0.0.1:$HGPORT '?cmd=heads'
  200 Script output follows
  
  cad8025a2e87f88c06259790adfa15acb4080123

branches

  $ get-with-headers.py 127.0.0.1:$HGPORT '?cmd=branches&nodes=0000000000000000000000000000000000000000'
  200 Script output follows
  
  0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000

changegroup

  $ get-with-headers.py 127.0.0.1:$HGPORT '?cmd=changegroup&roots=0000000000000000000000000000000000000000'
  200 Script output follows
  
  x\x9c\xbd\x94MHTQ\x14\xc7'+\x9d\xc66\x81\x89P\xc1\xa3\x14\xcct\xba\xef\xbe\xfb\xde\xbb\xcfr0\xb3"\x02\x11[%\x98\xdcO\xa7\xd2\x19\x98y\xd2\x07h"\x96\xa0e\xda\xa6lUY-\xca\x08\xa2\x82\x16\x96\xd1\xa2\xf0#\xc8\x95\x1b\xdd$!m*"\xc8\x82\xea\xbe\x9c\x01\x85\xc9\x996\x1d\xf8\xc1\xe3~\x9d\xff9\xef\x7f\xaf\xcf\xe7\xbb\x19\xfc4\xec^\xcb\x9b\xfbz\xa6\xbe\xb3\x90_\xef/\x8d\x9e\xad\xbe\xe4\xcb0\xd2\xec\xad\x12X:\xc8\x12\x12\xd9:\x95\xba	\x1cG\xb7$\xc5\xc44\x1c(\x1d\x03\x03\xdb\x84\x0cK#\xe0\x8a\xb8\x1b\x00\x1a\x08p\xb2SF\xa3\x01\x8f\x00%q\xa1Ny{k!8\xe5t>[{\xe2j\xddl\xc3\xcf\xee\xd0\xddW\x9ff3U\x9djobj\xbb\x87E\x88\x05l\x001\x12\x18\x13\xc6 \xb7(\xe3\x02a\x80\x81\xcel.u\x9b\x1b\x8c\x91\x80Z\x0c\x15\x15 (esc)
  \x7f0\xdc\xe4\x92\xa6\xb87\x16\xf2\xcaT\x14\xef\xe1\\pM\r (no-eol) (esc)
  kz\x10h2\x1a\xd3X\x98D\x9aD\\\xb8\x1a\x14\x12\x10f#\x87\xe8H\xad\x1d\xd9\xb2\xf5}cV{}\xf6:\xb3\xbd\xad\xaf\xd5?\xb9\xe3\xf6\xd4\xcf\x15\x84.\x8bT{\x97\x16\xa4Z\xeaX\x10\xabL\xc8\x81DJ\xc8\x18\x00\xccq\x80A-j2j	\x83\x1b\x02\x03O|PQ\xae\xc8W\x9d\xd7h\x8cDX\xb8<\xee\x12\xda,\xfe\xfc\x005\xb3K\xc1\x14\xd9\x8b\xb3^C\xc7\xa6\xb3\xea\x83\xdd\xdf.d\x17]\xe9\xbf\xff}\xe3\xf0#\xff\xaam+\x88Z\x16\xa9\xf6&tT+\xf2\x96\xe8h\x8d$\x94\xa8\xf1}\x8aC\x8a\xc2\xc59\x8dE[Z\x8e\xb9\xda\xc9cnX\x8b\xb467{\xad\x8e\x11\xe6\x8aX\xb9\x96L52\xbf\xb0\xff\xe3\x81M\x9fk\x07\xf3\x7f\xf4\x1c\xbe\xbc\x80s\xea^\x7fY\xc1\xca\xcb"\x8d\xbb\x1a\x16]\xea\x83\x82Cb8:$\x80Bd\x02\x08\x90!\x88P^\x12\x88B\xdba:\xa6\x0e\xe0<\xf0O\x8bU\x82\x81\xe3wr\xb2\xba\xe6{&\xcaNL\xceutln\xfb\xdc\xb6{,\xd3\x82\xd28IO\xb8\xd7G\x0cF!\x16\x86\x8d\x11@\x02A\xcb\xc2\x94Q\x04L\x01\x00u8\x86&0\xb0EtO\xd0\xc5\x9c#\xb4'\xef`\xc9\xaf\xd2\xd1\xf5\x83\xab\x9f<\x1e\x8fT\x84:R\x89L%\xe8/\xee \x8a>E\x99\xd7\x1dlZ\x08B\x1dc\xf5\\0\x83\x01B\x95Im\x1d[\x92s*\x99`L\xd7\x894e qfn\xb2 (esc)
  \xa5mh\xbc\xf8\xdd\xa9\xca\x9a*\xd9;^y\xd4\xf7t\xbah\xf5\xf9\x1b\x99\xfe\xe94\xcd*[zu\x05\x92\xa6ML\x82!D\x16"\xc0\x01\x90Y\xd2\x96\x08a\xe9\xdd\xfa\xa4\xb6\xc4#\xa6\xbexpjh\xa0$\xb7\xb0V\xdb\xfba\xbef\xee\xe1\xe9\x17\xbd\xfd3\x99JKc\xc25\x89+\xeaE\xce\xffK\x17>\xc7\xb7\x16tE^\x8e\xde\x0bu\x17Dg\x9e\xbf\x99\xd8\xf0\xa01\xd3\xbc+\xbc\x13k\x14~\x12\x89\xbaa\x11K\x96\xe5\xfb\r (no-eol) (esc)
  \x95)\xbe\xf6 (no-eol) (esc)

stream_out

  $ get-with-headers.py 127.0.0.1:$HGPORT '?cmd=stream_out'
  200 Script output follows
  
  1

failing unbundle, requires POST request

  $ get-with-headers.py 127.0.0.1:$HGPORT '?cmd=unbundle'
  405 push requires POST request
  
  0
  push requires POST request
  [1]

Static files

  $ get-with-headers.py 127.0.0.1:$HGPORT 'static/style.css'
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
    font-family: sans-serif;
    font-weight: bold;
  }
  .navigate a {
    background-color: #ccc;
    padding: 2pt;
    font-family: sans-serif;
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
  .logEntry th.label { width: 16em; }
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

Stop and restart with HGENCODING=cp932 and preferuncompressed

  $ killdaemons.py
  $ HGENCODING=cp932 hg serve --config server.preferuncompressed=True -n test \
  >     -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

commit message with Japanese Kanji 'Noh', which ends with '\x5c'

  $ echo foo >> foo
  $ HGENCODING=cp932 hg ci -m `$PYTHON -c 'print("\x94\x5c")'`

Graph json escape of multibyte character

  $ get-with-headers.py 127.0.0.1:$HGPORT 'graph/' > out
  >>> for line in open("out"):
  ...     if line.startswith("var data ="):
  ...         print line,
  var data = [["061dd13ba3c3", [0, 1], [[0, 0, 1, -1, ""]], "\u80fd", "test", "1970-01-01", ["unstable", true], ["tip"], ["something"]], ["cad8025a2e87", [0, 1], [[0, 0, 1, 3, "FF0000"]], "branch commit with null character: \u0000", "test", "1970-01-01", ["unstable", false], [], []], ["1d22e65f027e", [0, 1], [[0, 0, 1, 3, ""]], "branch", "test", "1970-01-01", ["stable", true], [], []], ["a4f92ed23982", [0, 1], [[0, 0, 1, 3, ""]], "Added tag 1.0 for changeset 2ef0ac749a14", "test", "1970-01-01", ["default", true], [], []], ["2ef0ac749a14", [0, 1], [], "base", "test", "1970-01-01", ["default", false], ["1.0"], ["anotherthing"]]];

capabilities

  $ get-with-headers.py 127.0.0.1:$HGPORT '?cmd=capabilities'; echo
  200 Script output follows
  
  lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch stream-preferred stream bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1*%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 (glob)

heads

ERRORS ENCOUNTERED

  $ cat errors.log
  $ killdaemons.py

  $ cd ..

Test graph paging

  $ mkcommit() {
  >  echo $1 >> a
  >  hg ci -Am $1 a
  > }

  $ hg init graph
  $ cd graph
  $ mkcommit 0
  $ mkcommit 1
  $ mkcommit 2
  $ mkcommit 3
  $ mkcommit 4
  $ mkcommit 5
  $ hg serve --config server.uncompressed=False \
  >          --config web.maxshortchanges=2 \
  >          -n test -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg log -G --template '{rev}:{node|short} {desc}\n'
  @  5:aed2d9c1d0e7 5
  |
  o  4:b60a39a85a01 4
  |
  o  3:ada793dcc118 3
  |
  o  2:ab4f1438558b 2
  |
  o  1:e06180cbfb0c 1
  |
  o  0:b4e73ffab476 0
  

Test paging

  $ get-with-headers.py 127.0.0.1:$HGPORT \
  >   'graph/?style=raw' | grep changeset
  changeset:   aed2d9c1d0e7
  changeset:   b60a39a85a01

  $ get-with-headers.py 127.0.0.1:$HGPORT \
  >   'graph/?style=raw&revcount=3' | grep changeset
  changeset:   aed2d9c1d0e7
  changeset:   b60a39a85a01
  changeset:   ada793dcc118

  $ get-with-headers.py 127.0.0.1:$HGPORT \
  >   'graph/e06180cbfb0?style=raw&revcount=3' | grep changeset
  changeset:   e06180cbfb0c
  changeset:   b4e73ffab476

  $ get-with-headers.py 127.0.0.1:$HGPORT \
  >   'graph/b4e73ffab47?style=raw&revcount=3' | grep changeset
  changeset:   b4e73ffab476

  $ cat errors.log

bookmarks view doesn't choke on bookmarks on secret changesets (issue3774)

  $ hg phase -fs 4
  $ hg bookmark -r4 secret
  $ cat > hgweb.cgi <<HGWEB
  > from mercurial import demandimport; demandimport.enable()
  > from mercurial.hgweb import hgweb
  > from mercurial.hgweb import wsgicgi
  > app = hgweb('.', 'test')
  > wsgicgi.launch(app)
  > HGWEB
  $ . "$TESTDIR/cgienv"
  $ PATH_INFO=/bookmarks; export PATH_INFO
  $ QUERY_STRING='style=raw'
  $ python hgweb.cgi | grep -v ETag:
  Status: 200 Script output follows\r (esc)
  Content-Type: text/plain; charset=ascii\r (esc)
  \r (esc)

listbookmarks hides secret bookmarks

  $ PATH_INFO=/; export PATH_INFO
  $ QUERY_STRING='cmd=listkeys&namespace=bookmarks'
  $ python hgweb.cgi
  Status: 200 Script output follows\r (esc)
  Content-Type: application/mercurial-0.1\r (esc)
  Content-Length: 0\r (esc)
  \r (esc)

search works with filtering

  $ PATH_INFO=/log; export PATH_INFO
  $ QUERY_STRING='rev=babar'
  $ python hgweb.cgi > search
  $ grep Status search
  Status: 200 Script output follows\r (esc)

summary works with filtering (issue3810)

  $ PATH_INFO=/summary; export PATH_INFO
  $ QUERY_STRING='style=monoblue'; export QUERY_STRING
  $ python hgweb.cgi > summary.out
  $ grep "^Status" summary.out
  Status: 200 Script output follows\r (esc)

proper status for filtered revision


(missing rev)

  $ PATH_INFO=/rev/5; export PATH_INFO
  $ QUERY_STRING='style=raw'
  $ python hgweb.cgi #> search
  Status: 404 Not Found\r (esc)
  ETag: *\r (glob) (esc)
  Content-Type: text/plain; charset=ascii\r (esc)
  \r (esc)
  
  error: filtered revision '5' (not in 'served' subset)



(filtered rev)

  $ PATH_INFO=/rev/4; export PATH_INFO
  $ QUERY_STRING='style=raw'
  $ python hgweb.cgi #> search
  Status: 404 Not Found\r (esc)
  ETag: *\r (glob) (esc)
  Content-Type: text/plain; charset=ascii\r (esc)
  \r (esc)
  
  error: filtered revision '4' (not in 'served' subset)

filtered '0' changeset

(create new root)
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'babar' > jungle
  $ hg add jungle
  $ hg ci -m 'Babar is in the jungle!'
  created new head
  $ hg graft 0::
  grafting 0:b4e73ffab476 "0"
  grafting 1:e06180cbfb0c "1"
  grafting 2:ab4f1438558b "2"
  grafting 3:ada793dcc118 "3"
  grafting 4:b60a39a85a01 "4" (secret)
  grafting 5:aed2d9c1d0e7 "5"
(turning the initial root secret (filtered))
  $ hg phase --force --secret 0
  $ PATH_INFO=/graph/; export PATH_INFO
  $ QUERY_STRING=''
  $ python hgweb.cgi | grep Status
  Status: 200 Script output follows\r (esc)
(check rendered revision)
  $ QUERY_STRING='style=raw'
  $ python hgweb.cgi | grep -v ETag
  Status: 200 Script output follows\r (esc)
  Content-Type: text/plain; charset=ascii\r (esc)
  \r (esc)
  
  # HG graph
  # Node ID 1d9b947fef1fbb382a95c11a8f5a67e9a10b5026
  # Rows shown 7
  
  changeset:   1d9b947fef1f
  user:        test
  date:        1970-01-01
  summary:     5
  branch:      default
  tag:         tip
  
  node:        (0, 0) (color 1)
  edge:        (0, 0) -> (0, 1) (color 1)
  
  changeset:   0cfd435fd222
  user:        test
  date:        1970-01-01
  summary:     4
  
  node:        (0, 1) (color 1)
  edge:        (0, 1) -> (0, 2) (color 1)
  
  changeset:   6768b9939e82
  user:        test
  date:        1970-01-01
  summary:     3
  
  node:        (0, 2) (color 1)
  edge:        (0, 2) -> (0, 3) (color 1)
  
  changeset:   05b0497fd125
  user:        test
  date:        1970-01-01
  summary:     2
  
  node:        (0, 3) (color 1)
  edge:        (0, 3) -> (0, 4) (color 1)
  
  changeset:   9c102df67cfb
  user:        test
  date:        1970-01-01
  summary:     1
  
  node:        (0, 4) (color 1)
  edge:        (0, 4) -> (0, 5) (color 1)
  
  changeset:   3ebcd7db11bf
  user:        test
  date:        1970-01-01
  summary:     0
  
  node:        (0, 5) (color 1)
  edge:        (0, 5) -> (0, 6) (color 1)
  
  changeset:   c5e9bd96ae01
  user:        test
  date:        1970-01-01
  summary:     Babar is in the jungle!
  
  node:        (0, 6) (color 1)
  
  



  $ cd ..

