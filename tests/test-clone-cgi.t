This is a test of the wire protocol over CGI-based hgweb.
initialize repository

  $ hg init test
  $ cd test
  $ echo a > a
  $ hg ci -Ama
  adding a
  $ cd ..
  $ cat >hgweb.cgi <<HGWEB
  > #
  > # An example CGI script to use hgweb, edit as necessary
  > import cgitb
  > cgitb.enable()
  > from mercurial import demandimport; demandimport.enable()
  > from mercurial.hgweb import hgweb
  > from mercurial.hgweb import wsgicgi
  > application = hgweb("test", "Empty test repository")
  > wsgicgi.launch(application)
  > HGWEB
  $ chmod 755 hgweb.cgi

try hgweb request

  $ . "$TESTDIR/cgienv"
  $ QUERY_STRING="cmd=changegroup&roots=0000000000000000000000000000000000000000"; export QUERY_STRING
  $ python hgweb.cgi >page1 2>&1
  $ python "$TESTDIR/md5sum.py" page1
  1f424bb22ec05c3c6bc866b6e67efe43  page1
