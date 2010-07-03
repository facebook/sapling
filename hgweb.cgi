#!/usr/bin/env python
#
# An example hgweb CGI script, edit as necessary
# See also http://mercurial.selenic.com/wiki/PublishingRepositories

# Path to repo or hgweb config to serve (see 'hg help hgweb')
config = "/path/to/repo/or/config"

# Uncomment and adjust if Mercurial is not installed system-wide:
#import sys; sys.path.insert(0, "/path/to/python/lib")

# Uncomment to send python tracebacks to the browser if an error occurs:
#import cgitb; cgitb.enable()

from mercurial import demandimport; demandimport.enable()
from mercurial.hgweb import hgweb, wsgicgi
application = hgweb(config)
wsgicgi.launch(application)
