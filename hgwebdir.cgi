#!/usr/bin/env python
#
# An example CGI script to export multiple hgweb repos, edit as necessary

import cgitb, sys
cgitb.enable()

# sys.path.insert(0, "/path/to/python/lib") # if not a system-wide install
from mercurial import hgweb

# The config file looks like this:
# [paths]
# virtual/path = /real/path
# virtual/path = /real/path

h = hgweb.hgwebdir("hgweb.config")
h.run()
