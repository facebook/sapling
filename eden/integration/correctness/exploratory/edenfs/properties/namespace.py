# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `namespace` property: an acked mutation found the state it needs.

The contracts gives every write, rename, unlink, mkdir, chmod, `sl rm`, and
`sl mv` group a requirement on the values just before its effect point (a
write needs a file, a rename a present source, an unlink a file, a mkdir
an absent path, a chmod a present path, `sl rm` a tracked matched path,
`sl mv` a tracked source), under `namespace.precondition`
scoped by the action. This collector declares those obligations; the consistency
engine decides each over every explanation of the run (an indeterminate
mutation's requirement binds only when the explanation lets it happen).
"""

from __future__ import annotations

from collections.abc import Mapping

from eden.integration.correctness.exploratory.core.effects import (
    normalize_path,
    ObligationRef,
)
from eden.integration.correctness.exploratory.core.property import (
    CheckInput,
    CollectorSink,
    path_subject,
    PropertyDef,
    RuleDef,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.edenfs.contracts.files import (
    PRECONDITION_RULE,
)

NAMESPACE = "namespace"
_MUTATIONS = frozenset(
    {"write", "rename", "unlink", "mkdir", "chmod", "sl_rm", "sl_mv"}
)


def collect_namespace(check_input: CheckInput, sink: CollectorSink) -> None:
    """Declare every contract precondition obligation, keyed by the mutation's input path."""
    spec = check_input.spec
    for ref in sorted(
        check_input.effects.contract_obligations(), key=ObligationRef.document
    ):
        if ref.rule_id != PRECONDITION_RULE:
            continue
        action = spec.action(ref.scope)
        payload = action.payload
        path = payload["path"] if "path" in payload else payload["source"]
        sink.obligation(
            PRECONDITION_RULE,
            ref.scope,
            subject=path_subject(normalize_path(path)),
            witnesses=(Witness("input-kind", action.kind),),
        )


NAMESPACE_RULES: Mapping[str, RuleDef] = {
    PRECONDITION_RULE: RuleDef(
        id=PRECONDITION_RULE,
        statement="An acked mutation's precondition (a write needs a file, a rename "
        "a present source, an unlink a file, a mkdir an absent path, a chmod a "
        "present path, `sl rm` a tracked matched path, `sl mv` a tracked source) "
        "holds just before its effect point; an indeterminate mutation's holds "
        "whenever it happened.",
        subject_kind=SubjectKind.PATH,
        identity_roles=("input-kind",),
    ),
}

NAMESPACE_PROPERTY = PropertyDef(
    id=NAMESPACE,
    collect=collect_namespace,
    rules=NAMESPACE_RULES,
)
