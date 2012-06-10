Tests if hgweb can run without touching sys.stdin, as is required
by the WSGI standard and strictly implemented by mod_wsgi.

  $ hg init repo
  $ cd repo
  $ echo foo > bar
  $ hg add bar
  $ hg commit -m "test"
  $ cat > request.py <<EOF
  > from mercurial import dispatch
  > from mercurial.hgweb.hgweb_mod import hgweb
  > from mercurial.ui import ui
  > from mercurial import hg
  > from StringIO import StringIO
  > import os, sys
  > 
  > class FileLike(object):
  >     def __init__(self, real):
  >         self.real = real
  >     def fileno(self):
  >         print >> sys.__stdout__, 'FILENO'
  >         return self.real.fileno()
  >     def read(self):
  >         print >> sys.__stdout__, 'READ'
  >         return self.real.read()
  >     def readline(self):
  >         print >> sys.__stdout__, 'READLINE'
  >         return self.real.readline()
  > 
  > sys.stdin = FileLike(sys.stdin)
  > errors = StringIO()
  > input = StringIO()
  > output = StringIO()
  > 
  > def startrsp(status, headers):
  >     print '---- STATUS'
  >     print status
  >     print '---- HEADERS'
  >     print [i for i in headers if i[0] != 'ETag']
  >     print '---- DATA'
  >     return output.write
  > 
  > env = {
  >     'wsgi.version': (1, 0),
  >     'wsgi.url_scheme': 'http',
  >     'wsgi.errors': errors,
  >     'wsgi.input': input,
  >     'wsgi.multithread': False,
  >     'wsgi.multiprocess': False,
  >     'wsgi.run_once': False,
  >     'REQUEST_METHOD': 'GET',
  >     'SCRIPT_NAME': '',
  >     'PATH_INFO': '',
  >     'QUERY_STRING': '',
  >     'SERVER_NAME': '127.0.0.1',
  >     'SERVER_PORT': os.environ['HGPORT'],
  >     'SERVER_PROTOCOL': 'HTTP/1.0'
  > }
  > 
  > i = hgweb('.')
  > i(env, startrsp)
  > print '---- ERRORS'
  > print errors.getvalue()
  > print '---- OS.ENVIRON wsgi variables'
  > print sorted([x for x in os.environ if x.startswith('wsgi')])
  > print '---- request.ENVIRON wsgi variables'
  > print sorted([x for x in i.repo.ui.environ if x.startswith('wsgi')])
  > EOF
  $ python request.py
  ---- STATUS
  200 Script output follows
  ---- HEADERS
  [('Content-Type', 'text/html; charset=ascii')]
  ---- DATA
  ---- ERRORS
  
  ---- OS.ENVIRON wsgi variables
  []
  ---- request.ENVIRON wsgi variables
  ['wsgi.errors', 'wsgi.input', 'wsgi.multiprocess', 'wsgi.multithread', 'wsgi.run_once', 'wsgi.url_scheme', 'wsgi.version']

  $ cd ..
