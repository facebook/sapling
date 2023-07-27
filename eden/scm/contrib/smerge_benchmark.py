# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


class SmartMerge3Text:
    def __init__(self, basetext, atext, btext, wordmerge=False):
        pass


"""
todo:
- 'sl clone' a Git repo
- find merge commits
- for each merge commit c
    - find p1 (master), p2 (local) commits and base (gca of p1 and p2)
    - for each updated file in p2
        - if it is in all the 3 commits
            - check merge_algorithm(f_p1, f_p2, f_base) == f_c
"""


def debugmergebenchmark():
    pass
