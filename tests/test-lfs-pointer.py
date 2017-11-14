from __future__ import absolute_import, print_function

import os
import sys

# make it runnable using python directly without run-tests.py
sys.path[0:0] = [os.path.join(os.path.dirname(__file__), '..')]

from hgext.lfs import pointer

def tryparse(text):
    r = {}
    try:
        r = pointer.deserialize(text)
        print('ok')
    except Exception as ex:
        print(ex)
    if r:
        text2 = r.serialize()
        if text2 != text:
            print('reconstructed text differs')
    return r

t = ('version https://git-lfs.github.com/spec/v1\n'
     'oid sha256:4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1'
     '258daaa5e2ca24d17e2393\n'
     'size 12345\n'
     'x-foo extra-information\n')

tryparse('')
tryparse(t)
tryparse(t.replace('git-lfs', 'unknown'))
tryparse(t.replace('v1\n', 'v1\n\n'))
tryparse(t.replace('sha256', 'ahs256'))
tryparse(t.replace('sha256:', ''))
tryparse(t.replace('12345', '0x12345'))
tryparse(t.replace('extra-information', 'extra\0information'))
tryparse(t.replace('extra-information', 'extra\ninformation'))
tryparse(t.replace('x-foo', 'x_foo'))
tryparse(t.replace('oid', 'blobid'))
tryparse(t.replace('size', 'size-bytes').replace('oid', 'object-id'))
