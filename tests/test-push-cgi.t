This is a test of the push wire protocol over CGI-based hgweb.

initialize repository

  $ hg init r
  $ cd r
  $ echo a > a
  $ hg ci -A -m "0"
  adding a
  $ echo '[web]' > .hg/hgrc
  $ echo 'allow_push = *' >> .hg/hgrc
  $ echo 'push_ssl = false' >> .hg/hgrc

create hgweb invocation script

  $ cat >hgweb.cgi <<HGWEB
  > import cgitb
  > cgitb.enable()
  > from mercurial import demandimport; demandimport.enable()
  > from mercurial.hgweb import hgweb
  > from mercurial.hgweb import wsgicgi
  > application = hgweb('.', 'test repository')
  > wsgicgi.launch(application)
  > HGWEB
  $ chmod 755 hgweb.cgi

test preparation

  $ . "$TESTDIR/cgienv"
  $ REQUEST_METHOD="POST"; export REQUEST_METHOD
  $ CONTENT_TYPE="application/octet-stream"; export CONTENT_TYPE
  $ hg bundle --all bundle.hg
  1 changesets found
  $ CONTENT_LENGTH=279; export CONTENT_LENGTH;

expect unsynced changes

  $ QUERY_STRING="cmd=unbundle&heads=0000000000000000000000000000000000000000"; export QUERY_STRING
  $ python hgweb.cgi <bundle.hg >page1 2>&1
  $ cat page1
  Status: 200 Script output follows\r (esc)
  Content-Type: application/mercurial-0.1\r (esc)
  Content-Length: 19\r (esc)
  \r (esc)
  0
  unsynced changes

successful force push

  $ QUERY_STRING="cmd=unbundle&heads=666f726365"; export QUERY_STRING
  $ python hgweb.cgi <bundle.hg >page2 2>&1
  $ cat page2
  Status: 200 Script output follows\r (esc)
  Content-Type: application/mercurial-0.1\r (esc)
  \r (esc)
  1
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files

successful push

  $ QUERY_STRING="cmd=unbundle&heads=f7b1eb17ad24730a1651fccd46c43826d1bbc2ac"; export QUERY_STRING
  $ python hgweb.cgi <bundle.hg >page3 2>&1
  $ cat page3
  Status: 200 Script output follows\r (esc)
  Content-Type: application/mercurial-0.1\r (esc)
  \r (esc)
  1
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files
