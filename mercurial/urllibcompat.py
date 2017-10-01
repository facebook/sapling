# urllibcompat.py - adapters to ease using urllib2 on Py2 and urllib on Py3
#
# Copyright 2017 Google, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

from . import pycompat

if pycompat.ispy3:

    def getfullurl(req):
        return req.full_url

    def gethost(req):
        return req.host

    def getselector(req):
        return req.selector

    def getdata(req):
        return req.data

    def hasdata(req):
        return req.data is not None
else:

    def gethost(req):
        return req.get_host()

    def getselector(req):
        return req.get_selector()

    def getfullurl(req):
        return req.get_full_url()

    def getdata(req):
        return req.get_data()

    def hasdata(req):
        return req.has_data()
