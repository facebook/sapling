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

  $ . "$TESTDIR/cgienv"
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
