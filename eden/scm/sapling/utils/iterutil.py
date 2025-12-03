# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


def mark_first(iterable):
    """Yield (item, is_first) tuples from an iterable.

    >>> iterable = [1, 2, 3, 4]
    >>> for i in mark_first(iterable):
    ...   print(i)
    ...
    (1, True)
    (2, False)
    (3, False)
    (4, False)
    """
    it = iter(iterable)
    first = True
    for a in it:
        yield a, first
        first = False


def mark_last(iterable):
    """Yield (item, is_last) tuples from an iterable.

    >>> iterable = [1, 2, 3, 4]
    >>> for i in mark_last(iterable):
    ...   print(i)
    ...
    (1, False)
    (2, False)
    (3, False)
    (4, True)
    """
    it = iter(iterable)
    for a in it:
        for b in it:
            yield a, False
            a = b
        yield a, True
