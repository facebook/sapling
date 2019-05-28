  $ setconfig extensions.treemanifest=!
#require no-msys # MSYS will translate web paths as if they were file paths

  $ cat << EOF >> $HGRCPATH
  > [format]
  > allowbundle1=True
  > EOF

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
  > from edenscm.mercurial import demandimport; demandimport.enable()
  > from edenscm.mercurial.hgweb import hgweb
  > from edenscm.mercurial.hgweb import wsgicgi
  > application = hgweb("test", "Empty test repository")
  > wsgicgi.launch(application)
  > HGWEB
  $ chmod 755 hgweb.cgi

try hgweb request

  $ . "$TESTDIR/cgienv"
  $ QUERY_STRING="cmd=changegroup&roots=0000000000000000000000000000000000000000"; export QUERY_STRING
  $ $PYTHON hgweb.cgi >page1
  devel-warn: using deprecated bundlev1 format
   at: */changegroup.py:* (makechangegroup) (glob)
  $ $PYTHON "$TESTDIR/md5sum.py" page1
  1f424bb22ec05c3c6bc866b6e67efe43  page1

make sure headers are sent even when there is no body

  $ QUERY_STRING="cmd=listkeys&namespace=nosuchnamespace" $PYTHON hgweb.cgi
  Status: 200 Script output follows\r (esc)
  Content-Type: application/mercurial-0.1\r (esc)
  Content-Length: 0\r (esc)
  \r (esc)
