# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""extension that does copytracing fast

Copy tracing is mainly used for automatically detecting renames in @Product@ commands like
`rebase`, `graft`, `amend` etc. For example, assuming we have a commit graph like below:

::

    D        # updates b.txt
    |
    C        # moves a.txt -> b.txt
    |
    | B      # updates a.txt
    |/
    A        # merge base

When we try to rebase commit `B` onto commit `D`, copy tracing will automatically
detect that `a.txt` was renamed io `b.txt` in commit `C` and `b.txt` exists in commit `D`,
so @Product@ will merge `a.txt` of commit `B` into `b.txt` of commit `D` instead of
prompting a message saying 'a.txt' is not in commit `D` and ask user to resolve the
conflict.

The copy tracing algorithm supports both @Product@ and Git format repositories, the difference
between them are:

::

    - @Product@ format: the rename information is stored in file's header.
    - Git format: there is no rename information stored in the repository, we
    need to compute a content-similarity-score for two files, if the similarity score is higher
    than a threshold, we treat them as a rename.


The following are configs to tune the behavior of copy tracing algorithm:

::

    [copytrace]
    # Whether to fallback to content similarity rename detection. This is used for
    # @Product@ format repositories in case users forgot to record rename information
    # with `@prog@ mv`.
    fallback-to-content-similarity = True

    # Maximum rename edit (`add`, `delete`) cost, if the edit cost of two files exceeds this
    # threshold, we will not treat them as a rename no matter what the content similarity is.
    max-edit-cost = 1000

    # Content similarity threhold for rename detection. The definition of "similarity"
    # between file `a` and file `b` is: (len(a.lines()) - edit_cost(a, b)) / len(a.lines())
    #   * 1.0 means exact match
    #   * 0.0 means not match at all
    similarity-threshold = 0.8

    # limits the number of commits in the source "branch" i. e. "branch".
    # that is rebased or merged. These are the commits from base up to csrc
    # (see _mergecopies docblock below).
    # copytracing can be too slow if there are too
    # many commits in this "branch".
    sourcecommitlimit = 100

    # whether to enable fast copytracing during amends
    enableamendcopytrace = True

    # how many previous commits to search through when looking for amend
    # copytrace data.
    amendcopytracecommitlimit = 100
"""
