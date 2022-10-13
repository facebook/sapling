from typing import NewType

# A bunch of commonly used type definitions.

PhabricatorDiffNumberWithD = \
    NewType('PhabricatorDiffNumberWithD', str)  # aka "D1234567"

GitHubNumber = NewType('GitHubNumber', int)  # aka 1234 (as in #1234)

# GraphQL ID that identifies Repository from GitHubb schema;
# aka MDExOlB1bGxSZXF1ZXN0MjU2NDM3MjQw
GitHubRepositoryId = NewType('GitHubRepositoryId', str)

# aka 12 (as in gh/ezyang/12/base)
GhNumber = NewType('GhNumber', str)

# Actually, sometimes we smuggle revs in here.  We shouldn't.
# We want to guarantee that they're full canonical revs so that
# you can do equality on them without fear.
# commit 3f72e04eeabcc7e77f127d3e7baf2f5ccdb148ee
GitCommitHash = NewType('GitCommitHash', str)

# tree 3f72e04eeabcc7e77f127d3e7baf2f5ccdb148ee
GitTreeHash = NewType('GitTreeHash', str)
