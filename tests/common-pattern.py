# common patterns in test at can safely be replaced
from __future__ import absolute_import

substitutions = [
    # list of possible compressions
    (br'zstd,zlib,none,bzip2',
     br'$USUAL_COMPRESSIONS$'
    ),
    # capabilities sent through http
    (br'bundlecaps=HG20%2Cbundle2%3DHG20%250A'
     br'changegroup%253D01%252C02%250A'
     br'digests%253Dmd5%252Csha1%252Csha512%250A'
     br'error%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250A'
     br'hgtagsfnodes%250A'
     br'listkeys%250A'
     br'phases%253Dheads%250A'
     br'pushkey%250A'
     br'remote-changegroup%253Dhttp%252Chttps',
     # (the replacement patterns)
     br'$USUAL_BUNDLE_CAPS$'
    ),
]
