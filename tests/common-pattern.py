# common patterns in test at can safely be replaced
from __future__ import absolute_import

import os


substitutions = [
    # list of possible compressions
    (br"(zstd,)?zlib,none,bzip2", br"$USUAL_COMPRESSIONS$"),
    # capabilities sent through http
    (
        br"bundlecaps=HG20%2Cbundle2%3DHG20%250A"
        br"bookmarks%250A"
        br"changegroup%253D01%252C02%250A"
        br"digests%253Dmd5%252Csha1%252Csha512%250A"
        br"error%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250A"
        br"hgtagsfnodes%250A"
        br"listkeys%250A"
        br"phases%253Dheads%250A"
        br"pushkey%250A"
        br"remote-changegroup%253Dhttp%252Chttps",
        # (the replacement patterns)
        br"$USUAL_BUNDLE_CAPS$",
    ),
    # bundle2 capabilities sent through ssh
    (
        br"bundle2=HG20%0A"
        br"bookmarks%0A"
        br"changegroup%3D01%2C02%0A"
        br"digests%3Dmd5%2Csha1%2Csha512%0A"
        br"error%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0A"
        br"hgtagsfnodes%0A"
        br"listkeys%0A"
        br"phases%3Dheads%0A"
        br"pushkey%0A"
        br"remote-changegroup%3Dhttp%2Chttps",
        # (replacement patterns)
        br"$USUAL_BUNDLE2_CAPS$",
    ),
    # HTTP log dates
    (br' - - \[\d\d/.../2\d\d\d \d\d:\d\d:\d\d] "GET', br' - - [$LOGDATE$] "GET'),
    # Windows has an extra '/' in the following lines that get globbed away:
    #   pushing to file:/*/$TESTTMP/r2 (glob)
    #   comparing with file:/*/$TESTTMP/r2 (glob)
    #   sub/maybelarge.dat: largefile 34..9c not available from
    #       file:/*/$TESTTMP/largefiles-repo (glob)
    (
        br"(.*file:/)/?(/\$TESTTMP.*)",
        lambda m: m.group(1) + b"*" + m.group(2) + b" (glob)",
    ),
]

# Various platform error strings, keyed on a common replacement string
_errors = {
    br"$ENOENT$": (
        # strerror()
        br"No such file or directory",
        # FormatMessage(ERROR_FILE_NOT_FOUND)
        br"The system cannot find the file specified",
    ),
    br"$ENOTDIR$": (
        # strerror()
        br"Not a directory",
        # FormatMessage(ERROR_PATH_NOT_FOUND)
        br"The system cannot find the path specified",
    ),
    br"$ECONNRESET$": (
        # strerror()
        br"Connection reset by peer",
        # FormatMessage(WSAECONNRESET)
        br"An existing connection was forcibly closed by the remote host",
    ),
    br"$EADDRINUSE$": (
        # strerror()
        br"Address already in use",
        # FormatMessage(WSAEADDRINUSE)
        br"Only one usage of each socket address"
        br" \(protocol/network address/port\) is normally permitted",
    ),
}

for replace, msgs in _errors.items():
    substitutions.extend((m, replace) for m in msgs)

# Output lines on Windows that can be autocorrected for '\' vs '/' path
# differences.
_winpathfixes = [
    # cloning subrepo s\ss from $TESTTMP/t/s/ss
    # cloning subrepo foo\bar from http://localhost:$HGPORT/foo/bar
    br"(?m)^cloning subrepo \S+\\.*",
    # pulling from $TESTTMP\issue1852a
    br"(?m)^pulling from \$TESTTMP\\.*",
    # pushing to $TESTTMP\a
    br"(?m)^pushing to \$TESTTMP\\.*",
    # pushing subrepo s\ss to $TESTTMP/t/s/ss
    br"(?m)^pushing subrepo \S+\\\S+ to.*",
    # moving d1\d11\a1 to d3/d11/a1
    br"(?m)^moving \S+\\.*",
    # d1\a: not recording move - dummy does not exist
    br"\S+\\\S+: not recording move .+",
    # reverting s\a
    br"(?m)^reverting (?!subrepo ).*\\.*",
    # saved backup bundle to
    #     $TESTTMP\test\.hg\strip-backup/443431ffac4f-2fc5398a-backup.hg
    br"(?m)^saved backup bundle to \$TESTTMP.*\.hg",
    # no changes made to subrepo s\ss since last push to ../tcc/s/ss
    br"(?m)^no changes made to subrepo \S+\\\S+ since.*",
    # changeset 5:9cc5aa7204f0: stuff/maybelarge.dat references missing
    #     $TESTTMP\largefiles-repo-hg\.hg\largefiles\76..38
    br"(?m)^changeset .* references (corrupted|missing) \$TESTTMP\\.*",
    # stuff/maybelarge.dat: largefile 76..38 not available from
    #     file:/*/$TESTTMP\largefiles-repo (glob)
    br".*: largefile \S+ not available from file:/\*/.+",
    # hgrc parse error (double escaped)
    br"(?m)^hg: parse error: \".*",
]

if os.name == "nt":
    substitutions.extend(
        [
            (s, lambda match: match.group().replace(b"\\", b"/").replace(b"//", b"/"))
            for s in _winpathfixes
        ]
    )
