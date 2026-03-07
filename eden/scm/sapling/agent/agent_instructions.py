# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


def get_agent_instructions() -> str:
    meta_agent_instructions = get_meta_agent_instructions()
    return meta_agent_instructions


def get_meta_agent_instructions() -> str:
    try:
        from .fb import meta_agent_instructions

        return meta_agent_instructions.META_AGENT_INSTRUCTIONS
    except ImportError:
        return ""
