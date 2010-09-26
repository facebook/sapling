This tests if CGI files from before d0db3462d568 still work.

  $ hg init test
  $ cat >hgweb.cgi <<HGWEB
  > #!/usr/bin/env python
  > #
  > # An example CGI script to use hgweb, edit as necessary
  > 
  > import cgitb, os, sys
  > cgitb.enable()
  > 
  > # sys.path.insert(0, "/path/to/python/lib") # if not a system-wide install
  > from mercurial import hgweb
  > 
  > h = hgweb.hgweb("test", "Empty test repository")
  > h.run()
  > HGWEB

  $ chmod 755 hgweb.cgi

  $ cat >hgweb.config <<HGWEBDIRCONF
  > [paths]
  > test = test
  > HGWEBDIRCONF

  $ cat >hgwebdir.cgi <<HGWEBDIR
  > #!/usr/bin/env python
  > #
  > # An example CGI script to export multiple hgweb repos, edit as necessary
  > 
  > import cgitb, sys
  > cgitb.enable()
  > 
  > # sys.path.insert(0, "/path/to/python/lib") # if not a system-wide install
  > from mercurial import hgweb
  > 
  > # The config file looks like this.  You can have paths to individual
  > # repos, collections of repos in a directory tree, or both.
  > #
  > # [paths]
  > # virtual/path = /real/path
  > # virtual/path = /real/path
  > #
  > # [collections]
  > # /prefix/to/strip/off = /root/of/tree/full/of/repos
  > #
  > # collections example: say directory tree /foo contains repos /foo/bar,
  > # /foo/quux/baz.  Give this config section:
  > #   [collections]
  > #   /foo = /foo
  > # Then repos will list as bar and quux/baz.
  > 
  > # Alternatively you can pass a list of ('virtual/path', '/real/path') tuples
  > # or use a dictionary with entries like 'virtual/path': '/real/path'
  > 
  > h = hgweb.hgwebdir("hgweb.config")
  > h.run()
  > HGWEBDIR

  $ chmod 755 hgwebdir.cgi

  $ DOCUMENT_ROOT="/var/www/hg"; export DOCUMENT_ROOT
  $ GATEWAY_INTERFACE="CGI/1.1"; export GATEWAY_INTERFACE
  $ HTTP_ACCEPT="text/xml,application/xml,application/xhtml+xml,text/html;q=0.9,text/plain;q=0.8,image/png,*/*;q=0.5"; export HTTP_ACCEPT
  $ HTTP_ACCEPT_CHARSET="ISO-8859-1,utf-8;q=0.7,*;q=0.7"; export HTTP_ACCEPT_CHARSET
  $ HTTP_ACCEPT_ENCODING="gzip,deflate"; export HTTP_ACCEPT_ENCODING
  $ HTTP_ACCEPT_LANGUAGE="en-us,en;q=0.5"; export HTTP_ACCEPT_LANGUAGE
  $ HTTP_CACHE_CONTROL="max-age=0"; export HTTP_CACHE_CONTROL
  $ HTTP_CONNECTION="keep-alive"; export HTTP_CONNECTION
  $ HTTP_HOST="hg.omnifarious.org"; export HTTP_HOST
  $ HTTP_KEEP_ALIVE="300"; export HTTP_KEEP_ALIVE
  $ HTTP_USER_AGENT="Mozilla/5.0 (X11; U; Linux x86_64; en-US; rv:1.8.0.4) Gecko/20060608 Ubuntu/dapper-security Firefox/1.5.0.4"; export HTTP_USER_AGENT
  $ PATH_INFO="/"; export PATH_INFO
  $ PATH_TRANSLATED="/var/www/hg/index.html"; export PATH_TRANSLATED
  $ QUERY_STRING=""; export QUERY_STRING
  $ REMOTE_ADDR="127.0.0.2"; export REMOTE_ADDR
  $ REMOTE_PORT="44703"; export REMOTE_PORT
  $ REQUEST_METHOD="GET"; export REQUEST_METHOD
  $ REQUEST_URI="/test/"; export REQUEST_URI
  $ SCRIPT_FILENAME="/home/hopper/hg_public/test.cgi"; export SCRIPT_FILENAME
  $ SCRIPT_NAME="/test"; export SCRIPT_NAME
  $ SCRIPT_URI="http://hg.omnifarious.org/test/"; export SCRIPT_URI
  $ SCRIPT_URL="/test/"; export SCRIPT_URL
  $ SERVER_ADDR="127.0.0.1"; export SERVER_ADDR
  $ SERVER_ADMIN="eric@localhost"; export SERVER_ADMIN
  $ SERVER_NAME="hg.omnifarious.org"; export SERVER_NAME
  $ SERVER_PORT="80"; export SERVER_PORT
  $ SERVER_PROTOCOL="HTTP/1.1"; export SERVER_PROTOCOL
  $ SERVER_SIGNATURE="<address>Apache/2.0.53 (Fedora) Server at hg.omnifarious.org Port 80</address>"; export SERVER_SIGNATURE
  $ SERVER_SOFTWARE="Apache/2.0.53 (Fedora)"; export SERVER_SOFTWARE

  $ python hgweb.cgi > page1
  $ python hgwebdir.cgi > page2

  $ PATH_INFO="/test/"
  $ PATH_TRANSLATED="/var/something/test.cgi"
  $ REQUEST_URI="/test/test/"
  $ SCRIPT_URI="http://hg.omnifarious.org/test/test/"
  $ SCRIPT_URL="/test/test/"
  $ python hgwebdir.cgi > page3

  $ grep -i error page1 page2 page3
  [1]
