# An example WSGI script for IIS/isapi-wsgi to export multiple hgweb repos
# Copyright 2010-2016 Sune Foldager <cyano@me.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# Requirements:
# - Python 2.7, preferably 64 bit
# - PyWin32 for Python 2.7 (32 or 64 bit)
# - Mercurial installed from source (python setup.py install) or download the
#   python module installer from https://www.mercurial-scm.org/wiki/Download
# - IIS 7 or newer
#
#
# Installation and use:
#
# - Download or clone the isapi-wsgi source and run python setup.py install.
#   https://github.com/hexdump42/isapi-wsgi
#
# - Create a directory to hold the shim dll, config files etc. This can reside
#   inside the standard IIS directory, C:\inetpub, or anywhere else. Copy this
#   script there.
#
# - Run this script (i.e. python hgwebdir_wsgi.py) to get a shim dll. The
#   shim is identical for all scripts, so you can just copy and rename one
#   from an earlier run, if you wish. The shim needs to reside in the same
#   directory as this script.
#
# - Start IIS manager and create a new app pool:
#   .NET CLR Version: No Managed Code
#   Advanced Settings: Enable 32 Bit Applications, if using 32 bit Python.
#   You can adjust the identity and maximum worker processes if you wish. This
#   setup works fine with multiple worker processes.
#
# - Create an IIS application where your hgwebdir is to be served from.
#   Assign it the app pool you just created and point its physical path to the
#   directory you created.
#
# - In the application, remove all handler mappings and setup a wildcard script
#   handler mapping of type IsapiModule with the shim dll as its executable.
#   This file MUST reside in the same directory as the shim. The easiest way
#   to do all this is to close IIS manager, place a web.config file in your
#   directory and start IIS manager again. The file should contain:
#
#   <?xml version="1.0" encoding="UTF-8"?>
#   <configuration>
#       <system.webServer>
#           <handlers accessPolicy="Read, Script">
#               <clear />
#               <add name="hgwebdir" path="*" verb="*" modules="IsapiModule"
#                    scriptProcessor="C:\your\directory\_hgwebdir_wsgi.dll"
#                    resourceType="Unspecified" requireAccess="None"
#                    preCondition="bitness64" />
#           </handlers>
#       </system.webServer>
#   </configuration>
#
#   Where "bitness64" should be replaced with "bitness32" for 32 bit Python.
#
# - Edit ISAPI And CGI Restrictions on the web server (global setting). Add a
#   restriction pointing to your shim dll and allow it to run.
#
# - Create a configuration file in your directory and adjust the configuration
#   variables below to match your needs. Example configuration:
#
#   [web]
#   style = gitweb
#   push_ssl = false
#   allow_push = *
#   encoding = utf8
#
#   [server]
#   validate = true
#
#   [paths]
#   repo1 = c:\your\directory\repo1
#   repo2 = c:\your\directory\repo2
#
# - Restart the web server and see if things are running.
#

# Configuration file location
hgweb_config = r'c:\your\directory\wsgi.config'

# Global settings for IIS path translation
path_strip = 0   # Strip this many path elements off (when using url rewrite)
path_prefix = 1  # This many path elements are prefixes (depends on the
                 # virtual path of the IIS application).

from __future__ import absolute_import
import sys

# Adjust python path if this is not a system-wide install
#sys.path.insert(0, r'C:\your\custom\hg\build\lib.win32-2.7')

# Enable tracing. Run 'python -m win32traceutil' to debug
if getattr(sys, 'isapidllhandle', None) is not None:
    import win32traceutil
    win32traceutil.SetupForPrint # silence unused import warning

import isapi_wsgi
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
