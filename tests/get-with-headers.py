#!/usr/bin/env python

__doc__ = """This does HTTP get requests given a host:port and path and returns
a subset of the headers plus the body of the result."""

import httplib, sys
headers = [h.lower() for h in sys.argv[3:]]
conn = httplib.HTTPConnection(sys.argv[1])
conn.request("GET", sys.argv[2])
response = conn.getresponse()
print response.status, response.reason
for h in headers:
    if response.getheader(h, None) is not None:
        print "%s: %s" % (h, response.getheader(h))
print
sys.stdout.write(response.read())

if 200 <= response.status <= 299:
    sys.exit(0)
sys.exit(1)
