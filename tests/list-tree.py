from __future__ import (
    absolute_import,
    print_function,
)

import argparse
import os

ap = argparse.ArgumentParser()
ap.add_argument('path', nargs='+')
opts = ap.parse_args()

def gather():
    for p in opts.path:
        if not os.path.exists(p):
            return
        if os.path.isdir(p):
            yield p + os.path.sep
            for dirpath, dirs, files in os.walk(p):
                for d in dirs:
                    yield os.path.join(dirpath, d) + os.path.sep
                for f in files:
                    yield os.path.join(dirpath, f)
        else:
            yield p

print('\n'.join(sorted(gather(), key=lambda x: x.replace(os.path.sep, '/'))))
