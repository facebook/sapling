This tests if hgweb and hgwebdir still work if the REQUEST_URI variable is
no longer passed with the request. Instead, SCRIPT_NAME and PATH_INFO
should be used from d74fc8dec2b4 onward to route the request.

  $ hg init repo
  $ cd repo
  $ echo foo > bar
  $ hg add bar
  $ hg commit -m "test"
  $ hg tip
  changeset:   0:61c9426e69fe
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  
  $ cat > request.py <<EOF
  > from __future__ import absolute_import
  > import os
  > import sys
  > from mercurial.hgweb import (
  >     hgweb,
  >     hgwebdir,
  > )
  > from mercurial import (
  >     util,
  > )
  > stringio = util.stringio
  > 
  > errors = stringio()
  > input = stringio()
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
  >     'PATH_INFO': '/',
  >     'SCRIPT_NAME': '',
  >     'SERVER_NAME': '127.0.0.1',
  >     'SERVER_PORT': os.environ['HGPORT'],
  >     'SERVER_PROTOCOL': 'HTTP/1.0'
  > }
  > 
  > def process(app):
  >     content = app(env, startrsp)
  >     sys.stdout.write(output.getvalue())
  >     sys.stdout.write(''.join(content))
  >     getattr(content, 'close', lambda : None)()
  >     print '---- ERRORS'
  >     print errors.getvalue()
  > 
  > output = stringio()
  > env['QUERY_STRING'] = 'style=atom'
  > process(hgweb('.', name='repo'))
  > 
  > output = stringio()
  > env['QUERY_STRING'] = 'style=raw'
  > process(hgwebdir({'repo': '.'}))
  > EOF
  $ python request.py
  ---- STATUS
  200 Script output follows
  ---- HEADERS
  [('Content-Type', 'application/atom+xml; charset=ascii')]
  ---- DATA
  <?xml version="1.0" encoding="ascii"?>
  <feed xmlns="http://www.w3.org/2005/Atom">
   <!-- Changelog -->
   <id>http://127.0.0.1:$HGPORT/</id>
   <link rel="self" href="http://127.0.0.1:$HGPORT/atom-log"/>
   <link rel="alternate" href="http://127.0.0.1:$HGPORT/"/>
   <title>repo Changelog</title>
   <updated>1970-01-01T00:00:00+00:00</updated>
  
   <entry>
    <title>[default] test</title>
    <id>http://127.0.0.1:$HGPORT/#changeset-61c9426e69fef294feed5e2bbfc97d39944a5b1c</id>
    <link href="http://127.0.0.1:$HGPORT/rev/61c9426e69fe"/>
    <author>
     <name>test</name>
     <email>&#116;&#101;&#115;&#116;</email>
    </author>
    <updated>1970-01-01T00:00:00+00:00</updated>
    <published>1970-01-01T00:00:00+00:00</published>
    <content type="xhtml">
  	<table xmlns="http://www.w3.org/1999/xhtml">
  	<tr>
  		<th style="text-align:left;">changeset</th>
  		<td>61c9426e69fe</td>
                </tr>
                <tr>
                                <th style="text-align:left;">branch</th>
                                <td>default</td>
                </tr>
                <tr>
                                <th style="text-align:left;">bookmark</th>
  		<td></td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">tag</th>
  		<td>tip</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;">user</th>
  		<td>&#116;&#101;&#115;&#116;</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">description</th>
  		<td>test</td>
  	</tr>
  	<tr>
  		<th style="text-align:left;vertical-align:top;">files</th>
  		<td>bar<br /></td>
  	</tr>
  	</table>
    </content>
   </entry>
  
  </feed>
  ---- ERRORS
  
  ---- STATUS
  200 Script output follows
  ---- HEADERS
  [('Content-Type', 'text/plain; charset=ascii')]
  ---- DATA
  
  /repo/
  
  ---- ERRORS
  

  $ cd ..
