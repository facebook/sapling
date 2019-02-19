#require serve

An attempt at more fully testing the hgweb web interface.
The following things are tested elsewhere and are therefore omitted:
- archive, tested in test-archive
- unbundle, tested in test-push-http
- changegroupsubset, tested in test-pull

  $ cat << EOF >> $HGRCPATH
  > [format]
  > usegeneraldelta=yes
  > [format]
  > allowbundle1=True
  > EOF

  $ setconfig ui.allowemptycommit=1

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
  $ hg bookmark stable
  $ hg ci -Ambranch
  $ hg bookmark unstable
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

  $ hg serve --config server.uncompressed=False -n test -p 0 --port-file $TESTTMP/.port -d --pid-file=hg.pid -E errors.log
  $ HGPORT=`cat $TESTTMP/.port`
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg log -G --template '{rev}:{node|short} {desc}\n'
  @  3:09cdda9ba925 branch commit with null character: \x00 (esc)
  |
  o  2:f1550bad5957 branch
  |
  o  1:a4f92ed23982 Added tag 1.0 for changeset 2ef0ac749a14
  |
  o  0:2ef0ac749a14 base
  

Logs and changes

  $ get-with-headers.py $LOCALIP:$HGPORT 'log/?style=atom'
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
    <title>[default] branch commit with null character: </title>
    <id>http://*/#changeset-09cdda9ba9259039f6c79df097ffae3c8fc4bac8</id> (glob)
    <link href="http://*/rev/09cdda9ba925"/> (glob)
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
       <td>09cdda9ba925</td>
      </tr>
      <tr>
       <th style="text-align:left;">branch</th>
       <td>default</td>
      </tr>
      <tr>
       <th style="text-align:left;">bookmark</th>
       <td>unstable</td>
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
    <title>branch</title>
    <id>http://*/#changeset-f1550bad59574fb7fdd810a04904b273b085c29f</id> (glob)
    <link href="http://*/rev/f1550bad5957"/> (glob)
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
       <td>f1550bad5957</td>
      </tr>
      <tr>
       <th style="text-align:left;">branch</th>
       <td></td>
      </tr>
      <tr>
       <th style="text-align:left;">bookmark</th>
       <td>stable</td>
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
     <table xmlns="http://www.w3.org/1999/xhtml">
      <tr>
       <th style="text-align:left;">changeset</th>
       <td>a4f92ed23982</td>
      </tr>
      <tr>
       <th style="text-align:left;">branch</th>
       <td></td>
      </tr>
      <tr>
       <th style="text-align:left;">bookmark</th>
       <td>something</td>
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
  $ get-with-headers.py $LOCALIP:$HGPORT 'log/?style=rss'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <rss version="2.0">
    <channel>
      <link>http://*:$HGPORT/</link> (glob)
      <language>en-us</language>
  
      <title>test Changelog</title>
      <description>test Changelog</description>
      <item>
      <title>[default] branch commit with null character: </title>
      <guid isPermaLink="true">http://*/rev/09cdda9ba925</guid> (glob)
      <link>http://*/rev/09cdda9ba925</link> (glob)
      <description>
      <![CDATA[
          <table>
              <tr>
                  <th style="text-align:left;">changeset</th>
                  <td>09cdda9ba925</td>
              </tr>
              <tr>
                  <th style="text-align:left;">branch</th>
                  <td>default</td>
              </tr>
              <tr>
                  <th style="text-align:left;">bookmark</th>
                  <td>unstable</td>
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
      ]]>
      </description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  <item>
      <title>branch</title>
      <guid isPermaLink="true">http://*/rev/f1550bad5957</guid> (glob)
      <link>http://*/rev/f1550bad5957</link> (glob)
      <description>
      <![CDATA[
          <table>
              <tr>
                  <th style="text-align:left;">changeset</th>
                  <td>f1550bad5957</td>
              </tr>
              <tr>
                  <th style="text-align:left;">branch</th>
                  <td></td>
              </tr>
              <tr>
                  <th style="text-align:left;">bookmark</th>
                  <td>stable</td>
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
      ]]>
      </description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  <item>
      <title>Added tag 1.0 for changeset 2ef0ac749a14</title>
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
                  <td></td>
              </tr>
              <tr>
                  <th style="text-align:left;">bookmark</th>
                  <td>something</td>
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
      ]]>
      </description>
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
      ]]>
      </description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  
    </channel>
  </rss> (no-eol)
  $ get-with-headers.py $LOCALIP:$HGPORT 'log/1/?style=atom'
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
     <table xmlns="http://www.w3.org/1999/xhtml">
      <tr>
       <th style="text-align:left;">changeset</th>
       <td>a4f92ed23982</td>
      </tr>
      <tr>
       <th style="text-align:left;">branch</th>
       <td></td>
      </tr>
      <tr>
       <th style="text-align:left;">bookmark</th>
       <td>something</td>
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
  $ get-with-headers.py $LOCALIP:$HGPORT 'log/1/?style=rss'
  200 Script output follows
  
  <?xml version="1.0" encoding="ascii"?>
  <rss version="2.0">
    <channel>
      <link>http://*:$HGPORT/</link> (glob)
      <language>en-us</language>
  
      <title>test Changelog</title>
      <description>test Changelog</description>
      <item>
      <title>Added tag 1.0 for changeset 2ef0ac749a14</title>
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
                  <td></td>
              </tr>
              <tr>
                  <th style="text-align:left;">bookmark</th>
                  <td>something</td>
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
      ]]>
      </description>
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
      ]]>
      </description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  
    </channel>
  </rss> (no-eol)
  $ get-with-headers.py $LOCALIP:$HGPORT 'log/1/foo/?style=atom'
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
  $ get-with-headers.py $LOCALIP:$HGPORT 'log/1/foo/?style=rss'
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
      <link>http://*:$HGPORT/log/2ef0ac749a14/foo</link> (glob)
      <description><![CDATA[base(websub)]]></description>
      <author>&#116;&#101;&#115;&#116;</author>
      <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
  </item>
  
    </channel>
  </rss>
  $ get-with-headers.py $LOCALIP:$HGPORT 'shortlog/'
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
  <a href="https://mercurial-scm.org/">
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
     <a href="/rev/09cdda9ba925">branch commit with null character: </a>
     <span class="phase">draft</span> <span class="branchhead">default</span> <span class="tag">tip</span> <span class="tag">unstable</span> 
    </td>
   </tr>
   <tr>
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description">
     <a href="/rev/f1550bad5957">branch</a>
     <span class="phase">draft</span> <span class="tag">stable</span> 
    </td>
   </tr>
   <tr>
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description">
     <a href="/rev/a4f92ed23982">Added tag 1.0 for changeset 2ef0ac749a14</a>
     <span class="phase">draft</span> <span class="tag">something</span> 
    </td>
   </tr>
   <tr>
    <td class="age">Thu, 01 Jan 1970 00:00:00 +0000</td>
    <td class="author">test</td>
    <td class="description">
     <a href="/rev/2ef0ac749a14">base</a>
     <span class="phase">draft</span> <span class="tag">1.0</span> <span class="tag">anotherthing</span> 
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
  
  
  
  </body>
  </html>
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'rev/0/'
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
   changeset 0:<a href="/rev/2ef0ac749a14">2ef0ac749a14</a>
   <span class="phase">draft</span> <span class="tag">1.0</span> <span class="tag">anotherthing</span> 
  </h3>
  
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" value="" /></p>
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
  
  
  </body>
  </html>
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'rev/1/?style=raw'
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
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=base'
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
  <a href="https://mercurial-scm.org/">
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
  
  <p><input name="rev" id="search1" type="text" size="30" value="base" /></p>
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
     <span class="phase">draft</span> <span class="tag">1.0</span> <span class="tag">anotherthing</span> 
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
  
  
  
  </body>
  </html>
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=stable&style=raw' | grep 'revision:'
  revision:    2

Search with revset syntax

  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=tip^&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID 09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  # Query "tip^"
  # Mode revset expression search
  
  changeset:   f1550bad59574fb7fdd810a04904b273b085c29f
  revision:    2
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     branch
  bookmark:    stable
  
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=last(all(),2)^&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID 09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  # Query "last(all(),2)^"
  # Mode revset expression search
  
  changeset:   f1550bad59574fb7fdd810a04904b273b085c29f
  revision:    2
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     branch
  bookmark:    stable
  
  changeset:   a4f92ed23982be056b9852de5dfe873eaac7f0de
  revision:    1
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     Added tag 1.0 for changeset 2ef0ac749a14
  bookmark:    something
  
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=last(all(,2)^&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID 09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  # Query "last(all(,2)^"
  # Mode literal keyword search
  
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=last(al(),2)^&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID 09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  # Query "last(al(),2)^"
  # Mode literal keyword search
  
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=bookmark(anotherthing)&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID 09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  # Query "bookmark(anotherthing)"
  # Mode revset expression search
  
  changeset:   2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  revision:    0
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     base
  tag:         1.0
  bookmark:    anotherthing
  
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=bookmark(abc)&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID 09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  # Query "bookmark(abc)"
  # Mode literal keyword search
  
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=deadbeef:&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID 09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  # Query "deadbeef:"
  # Mode literal keyword search
  
  

  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=user("test")&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID 09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  # Query "user("test")"
  # Mode revset expression search
  
  changeset:   09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  revision:    3
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     branch commit with null character: \x00 (esc)
  branch:      default
  tag:         tip
  bookmark:    unstable
  
  changeset:   f1550bad59574fb7fdd810a04904b273b085c29f
  revision:    2
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     branch
  bookmark:    stable
  
  changeset:   a4f92ed23982be056b9852de5dfe873eaac7f0de
  revision:    1
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     Added tag 1.0 for changeset 2ef0ac749a14
  bookmark:    something
  
  changeset:   2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  revision:    0
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     base
  tag:         1.0
  bookmark:    anotherthing
  
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=user("re:test")&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID 09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  # Query "user("re:test")"
  # Mode literal keyword search
  
  

File-related

  $ get-with-headers.py $LOCALIP:$HGPORT 'file/1/foo/?style=raw'
  200 Script output follows
  
  foo
  $ get-with-headers.py $LOCALIP:$HGPORT 'annotate/1/foo/?style=raw'
  200 Script output follows
  
  
  test@0: foo
  
  
  
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'file/1/?style=raw'
  200 Script output follows
  
  
  drwxr-xr-x da
  -rw-r--r-- 45 .hgtags
  -rw-r--r-- 4 foo
  
  
  $ hg log --template "{file_mods}\n" -r 1
  
  $ hg parents --template "{node|short}\n" -r 1
  2ef0ac749a14
  $ hg parents --template "{node|short}\n" -r 1 foo
  2ef0ac749a14

  $ get-with-headers.py $LOCALIP:$HGPORT 'file/1/foo'
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
  <a href="https://mercurial-scm.org/">
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
  <h3>
   view foo @ 1:<a href="/rev/a4f92ed23982">a4f92ed23982</a>
   <span class="phase">draft</span> <span class="tag">something</span> 
  </h3>
  
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" value="" /></p>
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
   <td class="author"><a href="/file/f1550bad5957/foo">f1550bad5957</a> </td>
  </tr>
  </table>
  
  <div class="overflow">
  <div class="sourcefirst linewraptoggle">line wrap: <a class="linewraplink" href="javascript:toggleLinewrap()">on</a></div>
  <div class="sourcefirst"> line source</div>
  <pre class="sourcelines stripes4 wrap bottomline"
       data-logurl="/log/1/foo"
       data-selectabletag="SPAN"
       data-ishead="0">
  
  <span id="l1">foo</span><a href="#l1"></a>
  </pre>
  </div>
  
  <script type="text/javascript" src="/static/followlines.js"></script>
  
  </div>
  </div>
  
  
  
  </body>
  </html>
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'filediff/0/foo/?style=raw'
  200 Script output follows
  
  
  diff -r 000000000000 -r 2ef0ac749a14 foo
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +foo
  
  
  
  

  $ get-with-headers.py $LOCALIP:$HGPORT 'filediff/1/foo/?style=raw'
  200 Script output follows
  
  
  
  
  
  

  $ hg log --template "{file_mods}\n" -r 2
  foo
  $ hg parents --template "{node|short}\n" -r 2
  a4f92ed23982
  $ hg parents --template "{node|short}\n" -r 2 foo
  2ef0ac749a14

  $ get-with-headers.py $LOCALIP:$HGPORT 'file/2/foo'
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>test: f1550bad5957 foo</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="https://mercurial-scm.org/">
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
  <h3>
   view foo @ 2:<a href="/rev/f1550bad5957">f1550bad5957</a>
   <span class="phase">draft</span> <span class="tag">stable</span> 
  </h3>
  
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" value="" /></p>
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
  <pre class="sourcelines stripes4 wrap bottomline"
       data-logurl="/log/2/foo"
       data-selectabletag="SPAN"
       data-ishead="1">
  
  <span id="l1">another</span><a href="#l1"></a>
  </pre>
  </div>
  
  <script type="text/javascript" src="/static/followlines.js"></script>
  
  </div>
  </div>
  
  
  
  </body>
  </html>
  


Overviews

  $ get-with-headers.py $LOCALIP:$HGPORT 'raw-tags'
  200 Script output follows
  
  tip	09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  1.0	2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  $ get-with-headers.py $LOCALIP:$HGPORT 'raw-branches'
  200 Script output follows
  
  default	09cdda9ba9259039f6c79df097ffae3c8fc4bac8	open
  $ get-with-headers.py $LOCALIP:$HGPORT 'raw-bookmarks'
  200 Script output follows
  
  unstable	09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  stable	f1550bad59574fb7fdd810a04904b273b085c29f
  something	a4f92ed23982be056b9852de5dfe873eaac7f0de
  anotherthing	2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  $ get-with-headers.py $LOCALIP:$HGPORT 'summary/?style=gitweb'
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
  <a href="https://mercurial-scm.org/" title="Mercurial" style="float: right;">Mercurial</a>
  <a href="/">Mercurial</a>  / summary
  </div>
  
  <div class="page_nav">
  <div>
  summary |
  <a href="/shortlog?style=gitweb">shortlog</a> |
  <a href="/log?style=gitweb">changelog</a> |
  <a href="/graph?style=gitweb">graph</a> |
  <a href="/tags?style=gitweb">tags</a> |
  <a href="/bookmarks?style=gitweb">bookmarks</a> |
  <a href="/branches?style=gitweb">branches</a> |
  <a href="/file?style=gitweb">files</a> |
  <a href="/help?style=gitweb">help</a>
  </div>
  
  <div class="search">
  <form id="searchform" action="/log">
  <input type="hidden" name="style" value="gitweb" />
  <input name="rev" type="text" value="" size="40" />
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  </div>
  </div>
  
  <div class="title">&nbsp;</div>
  <table cellspacing="0">
  <tr><td>description</td><td>unknown</td></tr>
  <tr><td>owner</td><td>&#70;&#111;&#111;&#32;&#66;&#97;&#114;&#32;&#60;&#102;&#111;&#111;&#46;&#98;&#97;&#114;&#64;&#101;&#120;&#97;&#109;&#112;&#108;&#101;&#46;&#99;&#111;&#109;&#62;</td></tr>
  <tr><td>last change</td><td class="date age">Thu, 01 Jan 1970 00:00:00 +0000</td></tr>
  </table>
  
  <div><a  class="title" href="/shortlog?style=gitweb">changes</a></div>
  <table cellspacing="0">
  
  <tr class="parity0">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><i>test</i></td>
  <td>
  <a class="list" href="/rev/09cdda9ba925?style=gitweb">
  <b>branch commit with null character: </b>
  <span class="logtags"><span class="phasetag" title="draft">draft</span> <span class="branchtag" title="default">default</span> <span class="tagtag" title="tip">tip</span> <span class="bookmarktag" title="unstable">unstable</span> </span>
  </a>
  </td>
  <td class="link" nowrap>
  <a href="/rev/09cdda9ba925?style=gitweb">changeset</a> |
  <a href="/file/09cdda9ba925?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity1">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><i>test</i></td>
  <td>
  <a class="list" href="/rev/f1550bad5957?style=gitweb">
  <b>branch</b>
  <span class="logtags"><span class="phasetag" title="draft">draft</span> <span class="bookmarktag" title="stable">stable</span> </span>
  </a>
  </td>
  <td class="link" nowrap>
  <a href="/rev/f1550bad5957?style=gitweb">changeset</a> |
  <a href="/file/f1550bad5957?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity0">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><i>test</i></td>
  <td>
  <a class="list" href="/rev/a4f92ed23982?style=gitweb">
  <b>Added tag 1.0 for changeset 2ef0ac749a14</b>
  <span class="logtags"><span class="phasetag" title="draft">draft</span> <span class="bookmarktag" title="something">something</span> </span>
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
  <span class="logtags"><span class="phasetag" title="draft">draft</span> <span class="tagtag" title="1.0">1.0</span> <span class="bookmarktag" title="anotherthing">anotherthing</span> </span>
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
  <td><a class="list" href="/rev/1.0?style=gitweb"><b>1.0</b></a></td>
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
  <td><a class="list" href="/rev/unstable?style=gitweb"><b>unstable</b></a></td>
  <td class="link">
  <a href="/rev/09cdda9ba925?style=gitweb">changeset</a> |
  <a href="/log/09cdda9ba925?style=gitweb">changelog</a> |
  <a href="/file/09cdda9ba925?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity1">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><a class="list" href="/rev/stable?style=gitweb"><b>stable</b></a></td>
  <td class="link">
  <a href="/rev/f1550bad5957?style=gitweb">changeset</a> |
  <a href="/log/f1550bad5957?style=gitweb">changelog</a> |
  <a href="/file/f1550bad5957?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity0">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><a class="list" href="/rev/something?style=gitweb"><b>something</b></a></td>
  <td class="link">
  <a href="/rev/a4f92ed23982?style=gitweb">changeset</a> |
  <a href="/log/a4f92ed23982?style=gitweb">changelog</a> |
  <a href="/file/a4f92ed23982?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="parity1">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td><a class="list" href="/rev/anotherthing?style=gitweb"><b>anotherthing</b></a></td>
  <td class="link">
  <a href="/rev/2ef0ac749a14?style=gitweb">changeset</a> |
  <a href="/log/2ef0ac749a14?style=gitweb">changelog</a> |
  <a href="/file/2ef0ac749a14?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="light"><td colspan="3"><a class="list" href="/bookmarks?style=gitweb">...</a></td></tr>
  </table>
  
  <div><a class="title" href="/branches?style=gitweb">branches</a></div>
  <table cellspacing="0">
  
  <tr class="parity0">
  <td class="age"><i class="age">Thu, 01 Jan 1970 00:00:00 +0000</i></td>
  <td class="open"><a class="list" href="/shortlog/default?style=gitweb"><b>default</b></a></td>
  <td class="link">
  <a href="/changeset/09cdda9ba925?style=gitweb">changeset</a> |
  <a href="/log/09cdda9ba925?style=gitweb">changelog</a> |
  <a href="/file/09cdda9ba925?style=gitweb">files</a>
  </td>
  </tr>
  <tr class="light">
    <td colspan="3"><a class="list" href="/branches?style=gitweb">...</a></td>
  </tr>
  </table>
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
  
  $ get-with-headers.py $LOCALIP:$HGPORT 'graph/?style=gitweb'
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
  <a href="https://mercurial-scm.org/" title="Mercurial" style="float: right;">Mercurial</a>
  <a href="/">Mercurial</a>  / graph
  </div>
  
  <div class="page_nav">
  <div>
  <a href="/summary?style=gitweb">summary</a> |
  <a href="/shortlog/tip?style=gitweb">shortlog</a> |
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
  | <a href="/graph/2ef0ac749a14?style=gitweb">(0)</a> <a href="/graph/tip?style=gitweb">tip</a> 
  </div>
  
  <div class="search">
  <form id="searchform" action="/log">
  <input type="hidden" name="style" value="gitweb" />
  <input name="rev" type="text" value="" size="40" />
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  </div>
  </div>
  
  <div class="title">&nbsp;</div>
  
  <noscript>The revision graph only works with JavaScript-enabled browsers.</noscript>
  
  <div id="wrapper">
  <ul id="nodebgs"></ul>
  <canvas id="graph"></canvas>
  <ul id="graphnodes"><li data-node="09cdda9ba925">
   <span class="desc">
    <a class="list" href="/rev/09cdda9ba925?style=gitweb"><b>branch commit with null character: </b></a>
   </span>
   <span class="logtags"><span class="phasetag" title="draft">draft</span> <span class="branchtag" title="default">default</span> <span class="tagtag" title="tip">tip</span> <span class="bookmarktag" title="unstable">unstable</span> </span>
   <span class="info">1970-01-01, by test</span>
  </li>
  <li data-node="f1550bad5957">
   <span class="desc">
    <a class="list" href="/rev/f1550bad5957?style=gitweb"><b>branch</b></a>
   </span>
   <span class="logtags"><span class="phasetag" title="draft">draft</span> <span class="bookmarktag" title="stable">stable</span> </span>
   <span class="info">1970-01-01, by test</span>
  </li>
  <li data-node="a4f92ed23982">
   <span class="desc">
    <a class="list" href="/rev/a4f92ed23982?style=gitweb"><b>Added tag 1.0 for changeset 2ef0ac749a14</b></a>
   </span>
   <span class="logtags"><span class="phasetag" title="draft">draft</span> <span class="bookmarktag" title="something">something</span> </span>
   <span class="info">1970-01-01, by test</span>
  </li>
  <li data-node="2ef0ac749a14">
   <span class="desc">
    <a class="list" href="/rev/2ef0ac749a14?style=gitweb"><b>base</b></a>
   </span>
   <span class="logtags"><span class="phasetag" title="draft">draft</span> <span class="tagtag" title="1.0">1.0</span> <span class="bookmarktag" title="anotherthing">anotherthing</span> </span>
   <span class="info">1970-01-01, by test</span>
  </li>
  </ul>
  </div>
  
  <script>
  var data = [{"edges": [[0, 0, 1, 3, ""]], "node": "09cdda9ba925", "vertex": [0, 1]}, {"edges": [[0, 0, 1, 3, ""]], "node": "f1550bad5957", "vertex": [0, 1]}, {"edges": [[0, 0, 1, 3, ""]], "node": "a4f92ed23982", "vertex": [0, 1]}, {"edges": [], "node": "2ef0ac749a14", "vertex": [0, 1]}];
  var graph = new Graph();
  graph.scale(39);
  
  graph.vertex = function(x, y, radius, color, parity, cur) {
  	Graph.prototype.vertex.apply(this, arguments);
  	return ['<li class="bg parity' + parity + '"></li>', ''];
  }
  
  graph.render(data);
  </script>
  
  <div class="extra_nav">
  <a href="/graph/tip?revcount=30&style=gitweb">less</a>
  <a href="/graph/tip?revcount=120&style=gitweb">more</a>
  | <a href="/graph/2ef0ac749a14?style=gitweb">(0)</a> <a href="/graph/tip?style=gitweb">tip</a> 
  </div>
  
  <script type="text/javascript">
      ajaxScrollInit(
              '/graph/%next%?graphtop=09cdda9ba9259039f6c79df097ffae3c8fc4bac8&style=gitweb',
              '', <!-- NEXTHASH
              function (htmlText, previousVal) {
                  var m = htmlText.match(/'(\w+)', <!-- NEXTHASH/);
                  return m ? m[1] : null;
              },
              '#wrapper',
              '<div class="%class%" style="text-align: center;">%text%</div>',
              'graph'
      );
  </script>
  
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

  $ get-with-headers.py $LOCALIP:$HGPORT 'graph/?style=raw'
  200 Script output follows
  
  
  # HG graph
  # Node ID 09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  # Rows shown 4
  
  changeset:   09cdda9ba925
  user:        test
  date:        1970-01-01
  summary:     branch commit with null character: \x00 (esc)
  branch:      default
  tag:         tip
  bookmark:    unstable
  
  node:        (0, 0) (color 1)
  edge:        (0, 0) -> (0, 1) (color 1)
  
  changeset:   f1550bad5957
  user:        test
  date:        1970-01-01
  summary:     branch
  bookmark:    stable
  
  node:        (0, 1) (color 1)
  edge:        (0, 1) -> (0, 2) (color 1)
  
  changeset:   a4f92ed23982
  user:        test
  date:        1970-01-01
  summary:     Added tag 1.0 for changeset 2ef0ac749a14
  bookmark:    something
  
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

  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=capabilities'; echo
  200 Script output follows
  
  lookup changegroupsubset branchmap pushkey known getbundle unbundlehash unbundlereplay batch $USUAL_BUNDLE2_CAPS$ unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 httpmediatype=0.1rx,0.1tx,0.2tx compression=zstd,zlib

heads

  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=heads'
  200 Script output follows
  
  09cdda9ba9259039f6c79df097ffae3c8fc4bac8

branches

  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=branches&nodes=0000000000000000000000000000000000000000'
  200 Script output follows
  
  0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000

changegroup

#if common-zlib
  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=changegroup&roots=0000000000000000000000000000000000000000'
  200 Script output follows
  
  x\x9c\xbdT]HTA\x14^\xd2\xb2\xd6\x1e (esc)
  "\x82z\xb8\x84B\x89ns\xe7\xce\xbdw\xc6L\xcc\xa2\xf0\xc5\xc4\x88(\xa8\x98_\xd7\xd4]\xd8\xbd\xd1\x0fH"\x9a`\x94\xdaKYD\x96\x11\xf4\x1fBEAe?P\xa4\x06\xf9$\x81\xbe\xe4\x83\xf4\x92\x12\x05\x12esi\x172Vw{\xe9\xc0\xc7\x1df\xce\x99\xf3\x9ds\xbf9\x81@\xe0Jh\xf2\x96w~\xc5\xf8\xd7c{\x9b\xf3DOga\xf4\xf8\xd6\x8e@\x86\x96&\xb6\\bE\x90#\x15rM\xa6L\x1b\x10b:\x8aaj[\x04*ba\xe0\xda\x90ce\x05=\x19\xf7\x82\xc0\x00AA7\xa8h4\xe8#\xc8h\\\xea[\xde_\x9d\x0e\r (no-eol) (esc)
  \x93\xe6\xa7\x0b\xeb\xceU\x8f\xed\xfb\xd9Vz\xe3\xf5\xe4X\xa6\xacS\xc5&\x8e\xd6\xfbp(u\x80\x0b F\x12c\xca9\x14\x0e\xe3B"\x0c00\xb9+\x94\xe9 (esc)
  \x8bs\x1a\xd4\xcePc\x93\xc6\xe2P\xb8\xc6\xa35q\x7f\xaf\xd4/Sc\xddf!\xa40\xf4\xb6a\x86\x80\xa1\xa21\x83\x87i\xa4F\xc6\xa5g@\xa9\x00\xe5."\xd4D\xda\xf7\xe6\xd4\xae\xdc\xdb{v\xefx\xf8cdYOEv_\xfc^\xeb\x8bK\xf3\x10\x9de\xa9b\xff,H\xb7\x948\x10\xebL\x88@\xaa\x14\xe4\x1c\x00N\x08\xb0\x98\xc3l\xce\x1ci	Kb\x90,\xa8X#\xdbo\xb7\xfen\xd4\xe0\x1a\x8bX\x8cFxX\xaf:\x97\x0c}\xb8p=\xbf\x93|{}q\xf2\xec\xcc\x9d\x92\x8eW\x8f\xdf\xccCa\x96\xa5\x8aMdu\x12.n\xa2\xa5y\xbf3\x1a<\xda\xd0P\xeb\x19\x87k\xbd\xb0\x119T_\xef71F\xb9'c\xc5F\xf2\xda'\x13\xd3\xdb>o_=U\xd5\xbd\xf2{\xfb\xce3\xd38\xa7\xfa\xcd\x97\xb9\x14\xfa\xb7\xa5\xd1\xcd\xbe\xdf\xfa\x0b@) \xb5\x88	)`\x10\xd9\x00\x02dI*\xb5J$b\xd0%\xdc\xc4\x8c\x00!\x82\xff\xe4\xac\x13t\x1d\xbc\x96\xb3\xa0e\xa2}\xb0\xa8nh\xbc\xa9iM\xe3Tc\xc9\xdbL\x0bJ\xa3\x113\xa1\xcb\x00\xb58\x83XZ.F\x00I\x04\x1d\x073\xce\x10\xb0%\x00\x8c\x08\x0cm`a\x87\x9a>\xa1S9{X\xfb\x8a\x8a\x82\x99\xc2\xfe\xa5\xddY\x0f\xee\x0fD6\x956\xa5"\x99\x8a\xd0\x1cJ\xa0\x1a\xa75\x8a\xfc\xee`\xdbA\x10\x9a\x18\xebA\xc0-\x0e(\xd3\xf2sM\xec(!\x98\xe2\x92s\xd3\xa4\xcaV\xc1\xc4\x9d\xb9\xc9*4\xb7\xde\x81u\x1f\x8f\x94U\x96\xab\x93\x03e\xfb\x03\x8fF\xf2\xb3N\\\xce\xf4O\xa7iV\xd1\x9f\x8fR"e\xbb\xd4\xa6\x18B\xe4  (esc)
  \x08\x80\xdcQ\xaeB\x08+\xff='\xb9%\xc6\x93^\x89\xd0poWAn^\x95\xb1\xe5\xd3D\xe5x\xdf\xd1\xe7';G3\xa5\x96F\x84\xc9\xe7\xe8[\xce\xffK\x17n\x15kW\xb5D^\xf6\xdf,m[\x15\x1d}\xf6np\xf9\xdd\x03\x99\xe6\x9dg&dk,\xa6\x91\xa8\x17\x96\xb1dY\x81_o'\xc4G (no-eol) (esc)
#endif

stream_out

  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=stream_out'
  200 Script output follows
  
  1

failing unbundle, requires POST request

  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=unbundle'
  405 push requires POST request
  
  0
  push requires POST request
  [1]

Static files

  $ get-with-headers.py $LOCALIP:$HGPORT 'static/style.css'
  200 Script output follows
  
  a { text-decoration:none; }
  .age { white-space:nowrap; }
  .date { white-space:nowrap; }
  .indexlinks { white-space:nowrap; }
  .parity0 { background-color: #ddd; color: #000; }
  .parity1 { background-color: #eee; color: #000; }
  .lineno { width: 60px; color: #aaa; font-size: smaller;
            text-align: right; }
  .plusline { color: green; }
  .minusline { color: red; }
  .atline { color: purple; }
  .annotate { font-size: smaller; text-align: right; padding-right: 1em; }
  tr.thisrev a { color:#999999; text-decoration: none; }
  tr.thisrev pre { color:#009900; }
  td.annotate {
    white-space: nowrap;
  }
  div.annotate-info {
    display: none;
    position: absolute;
    background-color: #FFFFFF;
    border: 1px solid #888;
    text-align: left;
    color: #000000;
    padding: 5px;
  }
  div.annotate-info a { color: #0000FF; }
  td.annotate:hover div.annotate-info { display: inline; }
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
  }

Stop and restart the server at the directory different from the repository
root. Even in such case, file patterns should be resolved relative to the
repository root. (issue4568)

  $ killdaemons.py
  $ hg serve --config server.preferuncompressed=True -n test \
  > -p $HGPORT -d --pid-file=`pwd`/hg.pid -E `pwd`/errors.log \
  > --cwd .. -R `pwd`
  $ cat hg.pid >> $DAEMON_PIDS

  $ get-with-headers.py $LOCALIP:$HGPORT 'log?rev=adds("foo")&style=raw'
  200 Script output follows
  
  
  # HG changesets search
  # Node ID 09cdda9ba9259039f6c79df097ffae3c8fc4bac8
  # Query "adds("foo")"
  # Mode revset expression search
  
  changeset:   2ef0ac749a14e4f57a5a822464a0902c6f7f448f
  revision:    0
  user:        test
  date:        Thu, 01 Jan 1970 00:00:00 +0000
  summary:     base
  tag:         1.0
  bookmark:    anotherthing
  
  

capabilities

(plain version to check the format)

  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=capabilities' | dd ibs=75 count=1 2> /dev/null; echo
  200 Script output follows
  
  lookup changegroupsubset branchmap pushkey known

(spread version to check the content)

  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=capabilities' | tr ' ' '\n'; echo
  200
  Script
  output
  follows
  
  lookup
  changegroupsubset
  branchmap
  pushkey
  known
  getbundle
  unbundlehash
  unbundlereplay
  batch
  stream-preferred
  streamreqs=generaldelta,revlogv1
  stream_option
  $USUAL_BUNDLE2_CAPS$
  unbundle=HG10GZ,HG10BZ,HG10UN
  httpheader=1024
  httpmediatype=0.1rx,0.1tx,0.2tx
  compression=*zlib (glob)

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

  $ get-with-headers.py $LOCALIP:$HGPORT \
  >   'graph/?style=raw' | grep changeset
  changeset:   aed2d9c1d0e7
  changeset:   b60a39a85a01

  $ get-with-headers.py $LOCALIP:$HGPORT \
  >   'graph/?style=raw&revcount=3' | grep changeset
  changeset:   aed2d9c1d0e7
  changeset:   b60a39a85a01
  changeset:   ada793dcc118

  $ get-with-headers.py $LOCALIP:$HGPORT \
  >   'graph/e06180cbfb0?style=raw&revcount=3' | grep changeset
  changeset:   e06180cbfb0c
  changeset:   b4e73ffab476

  $ get-with-headers.py $LOCALIP:$HGPORT \
  >   'graph/b4e73ffab47?style=raw&revcount=3' | grep changeset
  changeset:   b4e73ffab476

  $ cat errors.log

MSYS changes environment variables starting with '/' into 'C:/MinGW/msys/1.0',
which changes the status line to '400 no such method: C:'.

#if no-msys

bookmarks view doesn't choke on bookmarks on secret changesets (issue3774)

  $ hg phase -fs 4
  $ hg bookmark -r4 secret
  $ cat > hgweb.cgi <<HGWEB
  > from edenscm.mercurial import demandimport; demandimport.enable()
  > from edenscm.mercurial.hgweb import hgweb
  > from edenscm.mercurial.hgweb import wsgicgi
  > app = hgweb('.', 'test')
  > wsgicgi.launch(app)
  > HGWEB
  $ . "$TESTDIR/cgienv"
  $ PATH_INFO=/bookmarks; export PATH_INFO
  $ QUERY_STRING='style=raw'
  $ $PYTHON hgweb.cgi | grep -v ETag:
  Status: 200 Script output follows\r (esc)
  Content-Type: text/plain; charset=ascii\r (esc)
  \r (esc)

listbookmarks hides secret bookmarks

  $ PATH_INFO=/; export PATH_INFO
  $ QUERY_STRING='cmd=listkeys&namespace=bookmarks'
  $ $PYTHON hgweb.cgi
  Status: 200 Script output follows\r (esc)
  Content-Type: application/mercurial-0.1\r (esc)
  Content-Length: 0\r (esc)
  \r (esc)

search works with filtering

  $ PATH_INFO=/log; export PATH_INFO
  $ QUERY_STRING='rev=babar'
  $ $PYTHON hgweb.cgi > search
  $ grep Status search
  Status: 200 Script output follows\r (esc)

summary works with filtering (issue3810)

  $ PATH_INFO=/summary; export PATH_INFO
  $ QUERY_STRING='style=monoblue'; export QUERY_STRING
  $ $PYTHON hgweb.cgi > summary.out
  $ grep "^Status" summary.out
  Status: 200 Script output follows\r (esc)

proper status for filtered revision


(missing rev)

  $ PATH_INFO=/rev/5; export PATH_INFO
  $ QUERY_STRING='style=raw'
  $ $PYTHON hgweb.cgi #> search
  Status: 404 Not Found\r (esc)
  ETag: W/"*"\r (glob) (esc)
  Content-Type: text/plain; charset=ascii\r (esc)
  \r (esc)
  
  error: filtered revision '5' (not in 'served' subset)



(filtered rev)

  $ PATH_INFO=/rev/4; export PATH_INFO
  $ QUERY_STRING='style=raw'
  $ $PYTHON hgweb.cgi #> search
  Status: 404 Not Found\r (esc)
  ETag: W/"*"\r (glob) (esc)
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
  $ $PYTHON hgweb.cgi | grep Status
  Status: 200 Script output follows\r (esc)
(check rendered revision)
  $ QUERY_STRING='style=raw'
  $ $PYTHON hgweb.cgi | grep -v ETag
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
  
  
#endif


  $ cd ..

