# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Credits: the command design and configuration schema are based on
# Jujutsu's `jj fix` (https://github.com/jj-vcs/jj). Below is Jujutsu's
# original license:
#
# Copyright 2024 The Jujutsu Authors
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
# https://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""rewrite file content in commits with configured formatting tools.

This module implements ``sl fix``: it runs configured external tools
(formatters/fixers) over file contents in selected commits and rewrites
those commits with the formatted output. Multi-commit stacks are supported
via ``--source REV``; fixed paths propagate to draft descendants so each
level of the stack stays consistent.

Tools run with the repository root as their working directory. Each tool
reads the file content to be fixed from stdin and writes the fixed content
to stdout. A non-zero exit status aborts the entire ``sl fix`` invocation.

Each tool is one ``[fix.tools.<NAME>]`` section with two keys:

  command   JSON array of argv elements. The literal substring ``$path``
            inside any element is replaced with the repository-relative
            path of the file being formatted.
  patterns  JSON array of patterns (see ``sl help patterns``). All
            patterns are interpreted relative to the repository root.

Example::

    [fix.tools.black]
    command = ["black", "--quiet", "-", "--stdin-filename=$path"]
    patterns = ["glob:**/*.py"]

    [fix.tools.clang_format]
    command = ["clang-format", "--assume-filename=$path"]
    patterns = ["glob:**/*.cpp", "glob:**/*.hpp"]

If multiple tools match the same file, every matching tool is applied in
the order the sections appear; the first tool's stdout becomes the next
tool's stdin.

By default, only files modified or added in the target commits are run
through tools. ``--include-unchanged-files`` widens the file set to every
file matching the tool patterns, regardless of whether the file was
changed in the commit. This is useful when adopting a new formatter on a
previously-untouched subtree.

A commit whose only change is undone by a formatter (e.g. a commit that
lowercases a file run through an uppercasing formatter) is dropped from
the stack rather than left as a no-op rewrite of its parent.

Other configuration::

    [fix]
    enabled = True       # set to False to disable the command entirely
    workers = N          # number of parallel tool invocations per commit;
                         # defaults to os.cpu_count()
    max-file-size = SIZE # skip files larger than SIZE (e.g. ``2MB``);
                         # unset/0 means no cap
"""

import json

from . import error, match as matchmod, util
from .i18n import _


class FixTool:
    """A configured external formatting tool with a name, command, and matcher."""

    def __init__(self, name, command, matcher):
        self.name = name
        self.command = command
        self.matcher = matcher

    def matches(self, path):
        return self.matcher(path)


def _loadtools(ui, repo):
    """Parse [fix.tools.<NAME>] config sections into FixTool instances.

    Each section must define ``command`` (a JSON array of argv strings) and
    ``patterns`` (a JSON array of glob strings). Aborts if no tools are
    configured.
    """
    tools = []
    prefix = "fix.tools."
    for section in ui.configsections():
        if not section.startswith(prefix):
            continue
        name = section[len(prefix) :]
        if not name:
            raise error.ConfigError(
                _("invalid fix tool section [%s]: missing tool name") % section
            )
        command = _parsecommand(ui.config(section, "command"), section)
        patterns = _parsepatterns(ui.config(section, "patterns"), section)
        matcher = matchmod.match(repo.root, "", patterns, default="glob")
        tools.append(FixTool(name, command, matcher))
    if not tools:
        raise error.Abort(
            _("no fix tools configured"),
            hint=_(
                "configure sections like [fix.tools.NAME] with command and patterns"
            ),
        )
    return tools


def _parsecommand(rawvalue, section):
    """Parse a tool command from config. Requires a JSON array of strings."""
    if rawvalue is None:
        raise error.ConfigError(_("missing %s.command") % section)
    command = _parsearray(rawvalue, section, "command")
    if not command:
        raise error.ConfigError(_("empty %s.command") % section)
    return command


def _parsepatterns(rawvalue, section):
    """Parse file matching patterns from config. Expects a JSON string array."""
    if rawvalue is None:
        raise error.ConfigError(_("missing %s.patterns") % section)
    patterns = _parsearray(rawvalue, section, "patterns")
    if not patterns:
        raise error.ConfigError(_("empty %s.patterns") % section)
    return [_normalizepattern(pattern, section) for pattern in patterns]


def _parsearray(rawvalue, section, name):
    """Parse a JSON array of strings."""
    try:
        value = json.loads(rawvalue)
    except ValueError as exc:
        raise error.ConfigError(
            _("invalid %s.%s: %s") % (section, name, util.forcebytestr(exc))
        )
    if not isinstance(value, list):
        raise error.ConfigError(_("%s.%s must be a JSON array") % (section, name))
    for item in value:
        if not isinstance(item, str):
            raise error.ConfigError(
                _("%s.%s must contain only strings") % (section, name)
            )
    return value


def _normalizepattern(pattern, section):
    """Strip surrounding quotes from the value part of kind:value patterns.

    Config values like ``glob:'*.py'`` arrive with the quotes still embedded;
    the matcher expects ``glob:*.py``.
    """
    kind, sep, value = pattern.partition(":")
    if not sep:
        return pattern
    if not kind:
        raise error.ConfigError(
            _("invalid empty matcher kind in %s.patterns") % section
        )
    if len(value) >= 2 and value[0] == value[-1] and value[0] in ("'", '"'):
        return "%s:%s" % (kind, value[1:-1])
    return pattern
