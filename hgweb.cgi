#!/usr/bin/env python
#
# An example CGI script to use hgweb, edit as necessary

import cgitb, os, sys
cgitb.enable()

# sys.path.insert(0, "/path/to/python/lib") # if not a system-wide install
from mercurial.hgweb.hgweb_mod import hgweb
from mercurial.hgweb.request import wsgiapplication
import mercurial.hgweb.wsgicgi as wsgicgi

def make_web_app():
    return hgweb("/path/to/repo", "repository name")

wsgicgi.launch(wsgiapplication(make_web_app))
