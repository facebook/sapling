# common patterns in test at can safely be replaced
from __future__ import absolute_import

import os


substitutions = [
    # list of possible compressions
    (rb"(zstd,)?zlib,none,bzip2", rb"$USUAL_COMPRESSIONS$"),
    # capabilities sent through http
    (
        rb"bundlecaps=HG20%2Cbundle2%3DHG20%250A"
        rb"bookmarks%250A"
        rb"changegroup%253D01%252C02%250A"
        rb"digests%253Dmd5%252Csha1%252Csha512%250A"
        rb"error%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250A"
        rb"listkeys%250A"
        rb"phases%253Dheads%250A"
        rb"pushkey%250A"
        rb"remote-changegroup%253Dhttp%252Chttps",
        # (the replacement patterns)
        rb"$USUAL_BUNDLE_CAPS$",
    ),
    # bundle2 capabilities sent through ssh
    (
        rb"bundle2=HG20%0A"
        rb"bookmarks%0A"
        rb"changegroup%3D01%2C02%0A"
        rb"digests%3Dmd5%2Csha1%2Csha512%0A"
        rb"error%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0A"
        rb"listkeys%0A"
        rb"phases%3Dheads%0A"
        rb"pushkey%0A"
        rb"remote-changegroup%3Dhttp%2Chttps",
        # (replacement patterns)
        rb"$USUAL_BUNDLE2_CAPS$",
    ),
    # HTTP log dates
    (rb' - - \[\d\d/.../2\d\d\d \d\d:\d\d:\d\d] "GET', rb' - - [$LOGDATE$] "GET'),
    # Windows has an extra '/' in the following lines that get globbed away:
    #   pushing to file:/*/$TESTTMP/r2 (glob)
    #   comparing with file:/*/$TESTTMP/r2 (glob)
    #   sub/maybelarge.dat: largefile 34..9c not available from
    #       file:/*/$TESTTMP/largefiles-repo (glob)
    (
        rb"(.*file:/)/?(/\$TESTTMP.*)",
        lambda m: m.group(1) + b"*" + m.group(2) + b" (glob)",
    ),
]

# Various platform error strings, keyed on a common replacement string
_errors = {
    rb"$ENOENT$": (
        # strerror()
        rb"No such file or directory",
        # FormatMessage(ERROR_FILE_NOT_FOUND)
        rb"The system cannot find the file specified",
    ),
    rb"$ENOTDIR$": (
        # strerror()
        rb"Not a directory",
        # FormatMessage(ERROR_PATH_NOT_FOUND)
        rb"The system cannot find the path specified",
    ),
    rb"$ECONNRESET$": (
        # strerror()
        rb"Connection reset by peer",
        # FormatMessage(WSAECONNRESET)
        rb"An existing connection was forcibly closed by the remote host",
    ),
    rb"$EADDRINUSE$": (
        # strerror()
        rb"Address already in use",
        # FormatMessage(WSAEADDRINUSE)
        rb"Only one usage of each socket address"
        rb" \(protocol/network address/port\) is normally permitted",
    ),
}

for replace, msgs in _errors.items():
    substitutions.extend((m, replace) for m in msgs)

# Output lines on Windows that can be autocorrected for '\' vs '/' path
# differences.
_winpathfixes = [
    # cloning subrepo s\ss from $TESTTMP/t/s/ss
    # cloning subrepo foo\bar from http://localhost:$HGPORT/foo/bar
    rb"(?m)^cloning subrepo \S+\\.*",
    # pulling from $TESTTMP\issue1852a
    rb"(?m)^pulling from \$TESTTMP\\.*",
    # pushing to $TESTTMP\a
    rb"(?m)^pushing to \$TESTTMP\\.*",
    # pushing subrepo s\ss to $TESTTMP/t/s/ss
    rb"(?m)^pushing subrepo \S+\\\S+ to.*",
    # moving d1\d11\a1 to d3/d11/a1
    rb"(?m)^moving \S+\\.*",
    # d1\a: not recording move - dummy does not exist
    rb"\S+\\\S+: not recording move .+",
    # reverting s\a
    rb"(?m)^reverting (?!subrepo ).*\\.*",
    # no changes made to subrepo s\ss since last push to ../tcc/s/ss
    rb"(?m)^no changes made to subrepo \S+\\\S+ since.*",
    # changeset 5:9cc5aa7204f0: stuff/maybelarge.dat references missing
    #     $TESTTMP\largefiles-repo-hg\.hg\largefiles\76..38
    rb"(?m)^changeset .* references (corrupted|missing) \$TESTTMP\\.*",
    # stuff/maybelarge.dat: largefile 76..38 not available from
    #     file:/*/$TESTTMP\largefiles-repo (glob)
    rb".*: largefile \S+ not available from file:/\*/.+",
    # hgrc parse error (double escaped)
    rb"(?m)^hg: parse error: \".*",
]

if os.name == "nt":
    substitutions.extend(
        [
            (s, lambda match: match.group().replace(b"\\", b"/").replace(b"//", b"/"))
            for s in _winpathfixes
        ]
    )
