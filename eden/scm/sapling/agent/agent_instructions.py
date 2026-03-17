# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import base64

from sapling import error
from sapling.i18n import _


def get_agent_instructions(ui, names) -> str:
    key = "-".join(names)
    if config_overwrite := ui.config("help", key):
        if config_overwrite.startswith("base64:"):
            content = base64.b64decode(config_overwrite[7:])
            return content.decode("utf-8")
        else:
            return config_overwrite

    meta_agent_instructions = get_meta_agent_instructions(names, key)
    return meta_agent_instructions


def get_meta_agent_instructions(names, topic_key) -> str:
    try:
        from .fb import meta_agent_instructions
    except ImportError:
        return ""

    agent_advices = meta_agent_instructions.agent_advices
    # this is a temporary hack to support the old hotfix
    agent_advices["agent"] = meta_agent_instructions.META_AGENT_INSTRUCTIONS
    if topic_key not in agent_advices:
        raise error.Abort(_("agent instructions for '%s' not found") % " ".join(names))
    return agent_advices[topic_key]
