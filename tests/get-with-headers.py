#!/usr/bin/env python

"""This does HTTP GET requests given a host:port and path and returns
a subset of the headers plus the body of the result."""

import httplib, sys

try:
    import json
except ImportError:
    try:
        import simplejson as json
    except ImportError:
        json = None

try:
    import msvcrt, os
    msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
    msvcrt.setmode(sys.stderr.fileno(), os.O_BINARY)
except ImportError:
    pass

twice = False
if '--twice' in sys.argv:
    sys.argv.remove('--twice')
    twice = True
headeronly = False
if '--headeronly' in sys.argv:
    sys.argv.remove('--headeronly')
    headeronly = True
formatjson = False
if '--json' in sys.argv:
    sys.argv.remove('--json')
    formatjson = True

tag = None
def request(host, path, show):
    assert not path.startswith('/'), path
    global tag
    headers = {}
    if tag:
        headers['If-None-Match'] = tag

    conn = httplib.HTTPConnection(host)
    conn.request("GET", '/' + path, None, headers)
    response = conn.getresponse()
    print response.status, response.reason
    if show[:1] == ['-']:
        show = sorted(h for h, v in response.getheaders()
                      if h.lower() not in show)
    for h in [h.lower() for h in show]:
        if response.getheader(h, None) is not None:
            print "%s: %s" % (h, response.getheader(h))
    if not headeronly:
        print
        data = response.read()

        # Pretty print JSON. This also has the beneficial side-effect
        # of verifying emitted JSON is well-formed.
        if formatjson:
            if not json:
                print 'no json module not available'
                print 'did you forget a #require json?'
                sys.exit(1)

            # json.dumps() will print trailing newlines. Eliminate them
            # to make tests easier to write.
            data = json.loads(data)
            lines = json.dumps(data, sort_keys=True, indent=2).splitlines()
            for line in lines:
                print line.rstrip()
        else:
            sys.stdout.write(data)

        if twice and response.getheader('ETag', None):
            tag = response.getheader('ETag')

    return response.status

status = request(sys.argv[1], sys.argv[2], sys.argv[3:])
if twice:
    status = request(sys.argv[1], sys.argv[2], sys.argv[3:])

if 200 <= status <= 305:
    sys.exit(0)
sys.exit(1)
