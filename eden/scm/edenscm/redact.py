# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re

redactiontextregexs = [
    # This regex is looking for the patterns
    # starting with `oauth` or ending with `token` or `tokens`,
    # possibly in single or double quotes,
    # followed by either `:` or `=`
    # and then followed by a string of characters possibly in single or double quotes
    re.compile(
        r"""(oauth\s*[=:]|[a-z_-]*tokens?\s*[=:]|["']oauth["']\s*[=:]|["'][a-z_-]*tokens?["']\s*[=:])(\s*["']?)[a-zA-Z0-9-_%]+(["']?)""",
        re.I,
    ),
    # FB Token regex rule, as defined in SecretSearchFacebookTokenRule
    re.compile(r"""([^a-zA-Z0-9]|^)EAA[a-zA-Z0-9]{90,400}"""),
    # Github Token regex rule, as defined in SecretSearchGithubKeyRule
    re.compile(r"""gh[p|o|s|u|r]_[0-9a-zA-Z]{36}"""),
    # AWS Token regex rule, as defined in SecretSearchAWSAccessKeyRule
    re.compile(r"""KIA[A-Z0-9]{16}"""),
    # GCP API Token regex rule, as defined in SecretSearchGCPAPIKeyRule
    re.compile(r"""Iza[0-9A-Za-z-_]{35}"""),
]


def redactsensitiveinfo(string: str) -> str:
    """introspection of variables could potentially contain access tokens,
    make sure we never log those by replacing anything that looks like an access token

    >>> redactsensitiveinfo("prefix token=1234 suffix")
    'prefix <ACCESS_TOKEN_REDACTED> suffix'
    >>> redactsensitiveinfo("token: 1234\\ntoken: 1234")
    '<ACCESS_TOKEN_REDACTED>\\n<ACCESS_TOKEN_REDACTED>'
    >>> redactsensitiveinfo("access_token=1234")
    '<ACCESS_TOKEN_REDACTED>\\n<ACCESS_TOKEN_REDACTED>'
    >>> redactsensitiveinfo("'oauth'='1234'")
    '<ACCESS_TOKEN_REDACTED>
    """
    redacted = string
    for regex in redactiontextregexs:
        redacted = regex.sub("<ACCESS_TOKEN_REDACTED>", redacted)
    return redacted
