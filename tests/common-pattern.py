# common patterns in test at can safely be replaced
from __future__ import absolute_import

substitutions = [
    # list of possible compressions
    (br'(zstd,)?zlib,none,bzip2',
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
    # bundle2 capabilities sent through ssh
    (br'bundle2=HG20%0A'
     br'changegroup%3D01%2C02%0A'
     br'digests%3Dmd5%2Csha1%2Csha512%0A'
     br'error%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0A'
     br'hgtagsfnodes%0A'
     br'listkeys%0A'
     br'phases%3Dheads%0A'
     br'pushkey%0A'
     br'remote-changegroup%3Dhttp%2Chttps',
     # (replacement patterns)
     br'$USUAL_BUNDLE2_CAPS$'
    ),
    # HTTP log dates
    (br' - - \[\d\d/.../2\d\d\d \d\d:\d\d:\d\d] "GET',
     br' - - [$LOGDATE$] "GET'
    ),
]

# Various platform error strings, keyed on a common replacement string
_errors = {
    br'$ENOENT$': (
        # strerror()
        br'No such file or directory',

        # FormatMessage(ERROR_FILE_NOT_FOUND)
        br'The system cannot find the file specified',
    ),
    br'$ENOTDIR$': (
        # strerror()
        br'Not a directory',

        # FormatMessage(ERROR_PATH_NOT_FOUND)
        br'The system cannot find the path specified',
    ),
    br'$ECONNRESET$': (
        # strerror()
        br'Connection reset by peer',

        # FormatMessage(WSAECONNRESET)
        br'An existing connection was forcibly closed by the remote host',
    ),
    br'$EADDRINUSE$': (
        # strerror()
        br'Address already in use',

        # FormatMessage(WSAEADDRINUSE)
        br'Only one usage of each socket address'
        br' \(protocol/network address/port\) is normally permitted',
    ),
}

for replace, msgs in _errors.items():
    substitutions.extend((m, replace) for m in msgs)
