from __future__ import absolute_import

import os
import time

class mocktime(object):
    def __init__(self, increment):
        self.time = 0
        self.increment = [float(s) for s in increment.split()]
        self.pos = 0

    def __call__(self):
        self.time += self.increment[self.pos % len(self.increment)]
        self.pos += 1
        return self.time

def uisetup(ui):
    time.time = mocktime(os.environ.get('MOCKTIME', '0.1'))
