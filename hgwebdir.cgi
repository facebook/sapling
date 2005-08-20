#!/usr/bin/env python
#
# An example CGI script to export multiple hgweb repos, edit as necessary

import cgi, cgitb, os, sys, ConfigParser
cgitb.enable()

# sys.path.insert(0, "/path/to/python/lib") # if not a system-wide install
from mercurial import hgweb

# The config file looks like this:
# [paths]
# virtual/path = /real/path
# virtual/path = /real/path

h = hgweb.hgwebdir("hgweb.config")
h.run()
