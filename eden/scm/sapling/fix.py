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
