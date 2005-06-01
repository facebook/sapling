#!/usr/bin/env python
#
# An example CGI script to use hgweb, edit as necessary

import cgitb, os, sys
# sys.path.insert(0, "/path/to/python/lib") # if not a system-wide install
from mercurial import hgweb

h = hgweb.hgweb("/path/to/repo", "repository name")
h.run()
