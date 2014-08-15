# An example WSGI script for IIS/isapi-wsgi to export multiple hgweb repos
# Copyright 2010 Sune Foldager <cryo@cyanite.org>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# Requirements:
# - Python 2.6
# - PyWin32 build 214 or newer
# - Mercurial installed from source (python setup.py install)
# - IIS 7
#
# Earlier versions will in general work as well, but the PyWin32 version is
# necessary for win32traceutil to work correctly.
#
#
# Installation and use:
#
# - Download the isapi-wsgi source and run python setup.py install:
#   http://code.google.com/p/isapi-wsgi/
#
# - Run this script (i.e. python hgwebdir_wsgi.py) to get a shim dll. The
#   shim is identical for all scripts, so you can just copy and rename one
#   from an earlier run, if you wish.
#
# - Setup an IIS application where your hgwebdir is to be served from.
#   On 64-bit systems, make sure it's assigned a 32-bit app pool.
#
# - In the application, setup a wildcard script handler mapping of type
#   IsapiModule with the shim dll as its executable. This file MUST reside
#   in the same directory as the shim. Remove all other handlers, if you wish.
#
# - Make sure the ISAPI and CGI restrictions (configured globally on the
#   web server) includes the shim dll, to allow it to run.
#
# - Adjust the configuration variables below to match your needs.
#

# Configuration file location
hgweb_config = r'c:\src\iis\hg\hgweb.config'

# Global settings for IIS path translation
path_strip = 0   # Strip this many path elements off (when using url rewrite)
path_prefix = 1  # This many path elements are prefixes (depends on the
                 # virtual path of the IIS application).

import sys

# Adjust python path if this is not a system-wide install
#sys.path.insert(0, r'c:\path\to\python\lib')

# Enable tracing. Run 'python -m win32traceutil' to debug
if getattr(sys, 'isapidllhandle', None) is not None:
    import win32traceutil
    win32traceutil.SetupForPrint # silence unused import warning

# To serve pages in local charset instead of UTF-8, remove the two lines below
import os
os.environ['HGENCODING'] = 'UTF-8'


import isapi_wsgi
from mercurial import demandimport; demandimport.enable()
from mercurial.hgweb.hgwebdir_mod import hgwebdir

# Example tweak: Replace isapi_wsgi's handler to provide better error message
# Other stuff could also be done here, like logging errors etc.
class WsgiHandler(isapi_wsgi.IsapiWsgiHandler):
    error_status = '500 Internal Server Error' # less silly error message

isapi_wsgi.IsapiWsgiHandler = WsgiHandler

# Only create the hgwebdir instance once
application = hgwebdir(hgweb_config)

def handler(environ, start_response):

    # Translate IIS's weird URLs
    url = environ['SCRIPT_NAME'] + environ['PATH_INFO']
    paths = url[1:].split('/')[path_strip:]
    script_name = '/' + '/'.join(paths[:path_prefix])
    path_info = '/'.join(paths[path_prefix:])
    if path_info:
        path_info = '/' + path_info
    environ['SCRIPT_NAME'] = script_name
    environ['PATH_INFO'] = path_info

    return application(environ, start_response)

def __ExtensionFactory__():
    return isapi_wsgi.ISAPISimpleHandler(handler)

if __name__=='__main__':
    from isapi.install import ISAPIParameters, HandleCommandLine
    params = ISAPIParameters()
    HandleCommandLine(params)
