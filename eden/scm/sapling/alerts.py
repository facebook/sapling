# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from typing import List, NamedTuple, Optional

from . import cmdutil, templater
from .i18n import _


class Alert(NamedTuple):
    severity: str
    title: str
    url: str
    description: str
    key: str
    show_in_isl: bool
    show_after_crashes_regex: Optional[re.Pattern]


def parse_alert(ui, key: str, raw_alert: dict) -> Optional[Alert]:
    # make sure the basic fields are present
    severity = raw_alert["severity"].strip()
    title = raw_alert["title"].strip()
    description = raw_alert["description"].strip()
    url = raw_alert["url"].strip()
    if not severity or not title or not description or not url:
        return None

    show_in_isl = raw_alert.get("show-in-isl", True)
    reg = raw_alert.get("show-after-crashes-regex")

    show_after_crashes_regex = None
    if reg:
        try:
            show_after_crashes_regex = re.compile(reg)
        except Exception:
            pass
    return Alert(
        key=key,
        severity=severity,
        title=title,
        url=url,
        description=description,
        show_in_isl=show_in_isl,
        show_after_crashes_regex=show_after_crashes_regex,
    )


def get_alerts(ui) -> List[Alert]:
    raw_alerts = {}
    for name, value in ui.configitems("alerts"):
        try:
            key, field = name.rsplit(".", 1)
        except Exception:
            continue
        if not raw_alerts.get(key):
            raw_alerts[key] = {}
        raw_alerts[key][field] = value

    alerts = []
    for key, raw_alert in raw_alerts.items():
        alert = parse_alert(ui, key, raw_alert)
        if alert:
            alerts.append(alert)

    return alerts


def severity_to_color(severity: str) -> str:
    if severity == "SEV 0" or severity == "UBN":
        return "alerts.critical"
    elif severity == "SEV 1":
        return "alerts.high"
    elif severity == "SEV 2":
        return "alerts.medium"
    elif severity == "SEV 3":
        return "alerts.low"
    elif severity == "SEV 4":
        return "alerts.advice"
    return "bold"


def print_alert(ui, alert: Alert):
    tmpl_str = ui.config("templatealias", "alerts")
    if tmpl_str is None:
        return
    template = templater.unquotestring(tmpl_str)
    ui.write_err(
        cmdutil.rendertemplate(
            ui,
            template,
            {
                "severity": alert.severity,
                "severity_color": severity_to_color(alert.severity),
                "url": alert.url,
                "title": alert.title,
                "description": alert.description,
            },
        )
    )


def print_active_alerts(ui):
    alerts = get_alerts(ui)

    for alert in alerts:
        print_alert(ui, alert)


def print_matching_alerts_for_exception(ui, crash: str):
    if ui.plain():
        return

    related_alerts = []

    alerts = get_alerts(ui)
    for alert in alerts:
        if alert.show_after_crashes_regex and alert.show_after_crashes_regex.search(
            crash
        ):
            related_alerts.append(alert)

    if not related_alerts:
        return

    ui.write_err(_("This crash may be related to an ongoing issue:\n"))
    for alert in related_alerts:
        print_alert(ui, alert)
