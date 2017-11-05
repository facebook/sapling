# common patterns in test at can safely be replaced
from __future__ import absolute_import

substitutions = [
    # list of possible compressions
    (br'zstd,zlib,none,bzip2',
     br'$USUAL_COMPRESSIONS$'
    ),
]
