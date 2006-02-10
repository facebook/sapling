#!/usr/bin/env python
#
# An example CGI script to export multiple hgweb repos, edit as necessary

import cgitb, sys
cgitb.enable()

# sys.path.insert(0, "/path/to/python/lib") # if not a system-wide install
from mercurial import hgweb

# The config file looks like this.  You can have paths to individual
# repos, collections of repos in a directory tree, or both.
#
# [paths]
# virtual/path = /real/path
# virtual/path = /real/path
#
# [collections]
# /prefix/to/strip/off = /root/of/tree/full/of/repos
#
# collections example: say directory tree /foo contains repos /foo/bar,
# /foo/quux/baz.  Give this config section:
#   [collections]
#   /foo = /foo
# Then repos will list as bar and quux/baz.

# Alternatively you can pass a list of ('virtual/path', '/real/path') tuples
# or use a dictionary with entries like 'virtual/path': '/real/path'

h = hgweb.hgwebdir("hgweb.config")
h.run()
