# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import Tuple

from edenscm import error
from edenscm.i18n import _


def parse_username(username: str) -> Tuple[str, str]:
    r"""parses `username` and returns a tuple (name, email)

    username is expected to be the value of the ui.username config.
    The returned values should be able to be used with Git. Example:

    ```
    git -c user.name={name} -c user.email={email} commit ...
    ```

    Notes:
    - This function does not guarantee that `name` and `email` are free of
      characters that are disallowed by Git.
    - If it cannot find a non-empty value for `name`, it will raise a ValueError.
    - `email` may be the empty string. Note that Git accepts `"me <>"` as a
      valid author/committer value.

    >>> parse_username('Alyssa P. Hacker <alyssa@example.com>')
    ('Alyssa P. Hacker', 'alyssa@example.com')
    >>> parse_username('Alyssa P. Hacker')
    ('Alyssa P. Hacker', '')
    >>> parse_username('Alyssa P. Hacker <>')
    ('Alyssa P. Hacker', '')
    >>> parse_username('<alyssa@example.com>')
    ('alyssa', 'alyssa@example.com')
    >>> parse_username('<a@example.com>')
    ('a', 'a@example.com')
    >>> parse_username('<@example.com>')
    ('@example.com', '@example.com')
    >>> parse_username('   Alyssa P. Hacker   ')
    ('Alyssa P. Hacker', '')
    >>> parse_username('')
    Traceback (most recent call last):
     ...
    ValueError: Invalid username: ``
    """
    username = username.strip()
    email_start = username.rfind("<")
    if username.endswith(">") and email_start != -1:
        email = username[email_start + 1 : -1]
        name = username[:email_start]
    else:
        name = username
        email = ""
    name = name.strip()
    email = email.strip()
    if not name:
        if email:
            # Try extracting the username from the email to use as the name.
            at_index = email.find("@")
            if at_index > 0:
                name = email[:at_index]
            else:
                # Use email for both name and email in this case.
                name = email
        else:
            raise ValueError(f"Invalid username: `{username}`")
    return (name, email)


def normalize(userstr: str) -> str:
    """ensure the userstr contains '<>' for email, required by git"""
    if userstr.endswith(">") and " <" in userstr:
        return userstr
    else:
        return "%s <>" % userstr


def get_identity_or_raise(ui) -> Tuple[str, str]:
    """Returns a Git identity (name, email) based on ui.username or raises."""
    username = ui.config("ui", "username")
    if not username:
        raise error.Abort(
            _("ui.username not set. See %s for information on setting your identity.")
            % "https://sapling-scm.com/docs/introduction/getting-started"
        )
    return parse_username(username)
