# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import Tuple


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
    ValueError: invalid Git username: ``
    >>> parse_username('Alyssa <> Hacker')
    Traceback (most recent call last):
     ...
    ValueError: invalid '<' or '>' in Git username: `Alyssa <> Hacker`
    >>> parse_username('Alyssa < Hacker <alyssa@example.com>')
    Traceback (most recent call last):
     ...
    ValueError: invalid '<' or '>' in Git username: `Alyssa < Hacker <alyssa@example.com>`
    >>> parse_username('Alyssa Hacker <alyssa@example.com')
    Traceback (most recent call last):
     ...
    ValueError: invalid '<' or '>' in Git username: `Alyssa Hacker <alyssa@example.com`
    >>> parse_username('Alyssa Hacker alyssa@example.com>')
    Traceback (most recent call last):
     ...
    ValueError: invalid '<' or '>' in Git username: `Alyssa Hacker alyssa@example.com>`
    """
    username = username.strip()
    email_start = username.rfind("<")
    email_end = username.rfind(">")

    # Validate bad '<' or '>' at least since we make the user input them
    # manually (as opposed to Git which separates name/email).
    if (
        # more than 1 "<"
        email_start != username.find("<")
        # more than 1 ">"
        or email_end != username.find(">")
        # missing "<" or ">"
        or (email_start == -1) != (email_end == -1)
        # ">" before "<"
        or email_end < email_start
        # ">" not at end
        or (email_end != -1 and email_end != len(username) - 1)
    ):
        raise ValueError(f"invalid '<' or '>' in Git username: `{username}`")

    if email_start != -1:
        email = username[email_start + 1 : email_end]
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
            raise ValueError(f"invalid Git username: `{username}`")
    return (name, email)


def normalize(userstr: str) -> str:
    """ensure the userstr contains '<>' for email, required by git"""
    if userstr.endswith(">") and " <" in userstr:
        return userstr
    else:
        return "%s <>" % userstr


def get_identity_or_raise(ui) -> Tuple[str, str]:
    """Returns a Git identity (name, email) based on ui.username or raises."""
    return parse_username(ui.username())
