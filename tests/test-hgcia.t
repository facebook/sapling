Test the CIA extension

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > hgcia=
  > 
  > [hooks]
  > changegroup.cia = python:hgext.hgcia.hook
  > 
  > [web]
  > baseurl = http://hgserver/
  > 
  > [cia]
  > user = testuser
  > project = testproject
  > test = True
  > EOF

  $ hg init src
  $ hg init cia
  $ cd src
  $ echo foo > foo
  $ hg ci -Amfoo
  adding foo
  $ hg push ../cia
  pushing to ../cia
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  
  <message>
    <generator>
      <name>Mercurial (hgcia)</name>
      <version>0.1</version>
      <url>http://hg.kublai.com/mercurial/hgcia</url>
      <user>testuser</user>
    </generator>
    <source>
  <project>testproject</project>
  <branch>default</branch>
  </source>
    <body>
      <commit>
        <author>test</author>
        <version>0:e63c23eaa88a</version>
        <log>foo</log>
        <url>http://hgserver/rev/e63c23eaa88a</url>
        <files><file uri="http://hgserver/file/e63c23eaa88a/foo" action="add">foo</file></files>
      </commit>
    </body>
    <timestamp>0</timestamp>
  </message>

  $ cat >> $HGRCPATH <<EOF
  > strip = 0
  > EOF

  $ echo bar > bar
  $ hg ci -Ambar
  adding bar
  $ hg push ../cia
  pushing to ../cia
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  
  <message>
    <generator>
      <name>Mercurial (hgcia)</name>
      <version>0.1</version>
      <url>http://hg.kublai.com/mercurial/hgcia</url>
      <user>testuser</user>
    </generator>
    <source>
  <project>testproject</project>
  <branch>default</branch>
  </source>
    <body>
      <commit>
        <author>test</author>
        <version>1:c0c7cf58edc5</version>
        <log>bar</log>
        <url>http://hgserver/$TESTTMP/cia/rev/c0c7cf58edc5</url>
        <files><file uri="http://hgserver/$TESTTMP/cia/file/c0c7cf58edc5/bar" action="add">bar</file></files>
      </commit>
    </body>
    <timestamp>0</timestamp>
  </message>

  $ cd ..
