// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use hlua::{AnyLuaString, AnyLuaValue};
use lazy_static::lazy_static;
use regex::{Regex, RegexBuilder};
use std::collections::{HashMap, HashSet};
use std::mem;

const TITLE: &'static str = "title";
const CC: &'static str = "cc";
const SUBSCRIBERS: &'static str = "subscribers";
const DIFFERENTIAL_REVISION: &'static str = "differential revision";
const REVERT_PLAN: &'static str = "revert plan";
const REVIEWED_BY: &'static str = "reviewed by";
const REVIEWERS: &'static str = "reviewers";
const SUMMARY: &'static str = "summary";
const SIGNATURE: &'static str = "signature";
const TASKS: &'static str = "tasks";
const TEST_PLAN: &'static str = "test plan";

lazy_static! {
    static ref PHABRICATOR_TAGS: HashSet<&'static str> = {
        // This is a way to ensure that all the fields of the PhabricatorMessage have their
        // tags in this HashSet
        let PhabricatorMessage {
            title,
            cc,
            subscribers,
            differential_revision,
            revert_plan,
            reviewed_by,
            reviewers,
            summary,
            signature,
            tasks,
            test_plan,
        } = PhabricatorMessage::default();

        let mut tags = HashSet::new();
        if title.is_none() {
            // nothing to do, there is no "title" tag
        }

        if cc.is_none() {
            tags.insert(CC);
        }
        if subscribers.is_none() {
            tags.insert(SUBSCRIBERS);
        }
        if differential_revision.is_none() {
            tags.insert(DIFFERENTIAL_REVISION);
        }
        if revert_plan.is_none() {
            tags.insert(REVERT_PLAN);
        }
        if reviewed_by.is_none() {
            tags.insert(REVIEWED_BY);
        }
        if reviewers.is_none() {
            tags.insert(REVIEWERS);
        }
        if summary.is_none() {
            tags.insert(SUMMARY);
        }
        if signature.is_none() {
            tags.insert(SIGNATURE);
        }
        if tasks.is_none() {
            tags.insert(TASKS);
        }
        if test_plan.is_none() {
            tags.insert(TEST_PLAN);
        }

        tags
    };

    static ref SPLIT_USERNAMES: Regex = RegexBuilder::new(r"[\s,]+")
        .case_insensitive(true)
        .build()
        .unwrap();
}

#[derive(Clone, Default, Debug, Eq, PartialEq)]
pub struct PhabricatorMessage {
    pub title: Option<String>,
    pub cc: Option<Vec<String>>,
    pub subscribers: Option<Vec<String>>,
    pub differential_revision: Option<String>,
    pub revert_plan: Option<String>,
    pub reviewed_by: Option<Vec<String>>,
    pub reviewers: Option<Vec<String>>,
    pub summary: Option<String>,
    pub signature: Option<String>,
    pub tasks: Option<Vec<String>>,
    pub test_plan: Option<String>,
}

impl PhabricatorMessage {
    pub fn parse_message(msg: &str) -> Self {
        let lines = msg.lines();
        let mut parsed = PhabricatorMessage::default();

        let mut current_tag = "title".to_string();
        let mut current_value = Vec::new();

        for line in lines {
            let (maybe_tag, maybe_value) = {
                let mut maybe_tag_name_and_value = line.splitn(2, ":");
                (
                    maybe_tag_name_and_value
                        .next()
                        .map(|tag| tag.to_lowercase()),
                    maybe_tag_name_and_value.next(),
                )
            };

            match maybe_tag {
                Some(ref tag) if PHABRICATOR_TAGS.contains(tag.as_str()) => parsed.add(
                    mem::replace(&mut current_tag, tag.to_string()),
                    mem::replace(&mut current_value, vec![maybe_value.unwrap_or("")]),
                ),
                _ => current_value.push(line),
            }
        }
        parsed.add(current_tag, current_value);

        parsed
    }

    pub fn to_lua(self) -> HashMap<&'static str, AnyLuaValue> {
        let mut map = HashMap::new();
        let PhabricatorMessage {
            title,
            cc,
            subscribers,
            differential_revision,
            revert_plan,
            reviewed_by,
            reviewers,
            summary,
            signature,
            tasks,
            test_plan,
        } = self;

        let insert_str = |map: &mut HashMap<&str, AnyLuaValue>, name, value| {
            if let Some(v) = value {
                map.insert(name, to_lua_string(v));
            }
        };

        let insert_array = |map: &mut HashMap<&str, AnyLuaValue>, name, value| {
            if let Some(v) = value {
                map.insert(name, to_lua_array(v));
            }
        };

        insert_str(&mut map, TITLE, title);
        insert_array(&mut map, CC, cc);
        insert_array(&mut map, SUBSCRIBERS, subscribers);
        insert_str(&mut map, DIFFERENTIAL_REVISION, differential_revision);
        insert_str(&mut map, REVERT_PLAN, revert_plan);
        insert_array(&mut map, REVIEWED_BY, reviewed_by);
        insert_array(&mut map, REVIEWERS, reviewers);
        insert_str(&mut map, SUMMARY, summary);
        insert_str(&mut map, SIGNATURE, signature);
        insert_array(&mut map, TASKS, tasks);
        insert_str(&mut map, TEST_PLAN, test_plan);

        map
    }

    fn add(&mut self, tag: String, value: Vec<&str>) {
        let value = itertools::join(value, "\n").trim().to_string();

        let to_vec = |value: String| -> Vec<String> {
            SPLIT_USERNAMES
                .split(&value)
                .filter_map(|s| {
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.to_string())
                    }
                })
                .collect()
        };

        match tag.as_str() {
            TITLE => self.title = Some(value),
            CC => self.cc = Some(to_vec(value)),
            SUBSCRIBERS => self.subscribers = Some(to_vec(value)),
            DIFFERENTIAL_REVISION => self.differential_revision = Some(value),
            REVERT_PLAN => self.revert_plan = Some(value),
            REVIEWED_BY => self.reviewed_by = Some(to_vec(value)),
            REVIEWERS => self.reviewers = Some(to_vec(value)),
            SUMMARY => self.summary = Some(value),
            SIGNATURE => self.signature = Some(value),
            TASKS => self.tasks = Some(to_vec(value)),
            TEST_PLAN => self.test_plan = Some(value),
            bad => panic!("Unexpected phabricator tag {}, shouldn't happen", bad),
        }
    }
}

fn to_lua_string(s: String) -> AnyLuaValue {
    AnyLuaValue::LuaAnyString(AnyLuaString(s.as_bytes().to_vec()))
}

fn to_lua_array<'a, T: IntoIterator<Item = String>>(v: T) -> AnyLuaValue {
    let v: Vec<_> = v
        .into_iter()
        .enumerate()
        .map(|(i, val)| {
            (
                AnyLuaValue::LuaNumber((i + 1) as f64),
                AnyLuaValue::LuaString(val),
            )
        })
        .collect();
    AnyLuaValue::LuaArray(v)
}

#[cfg(test)]
mod test {
    use super::*;
    use maplit::hashmap;

    fn s(v: &str) -> String {
        v.to_string()
    }

    fn ss(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn test_parse_commit_msg() {
        fn check_parse_commit(
            commit_msg: &str,
            expected_msg: PhabricatorMessage,
            expected_lua: HashMap<&'static str, AnyLuaValue>,
        ) {
            let msg = PhabricatorMessage::parse_message(commit_msg);
            assert_eq!(msg, expected_msg);
            assert_eq!(msg.to_lua(), expected_lua);
        }

        check_parse_commit(
            "mononoke: fix bug\nSummary: fix\nTest Plan: testinprod",
            PhabricatorMessage {
                title: ss("mononoke: fix bug"),
                summary: ss("fix"),
                test_plan: ss("testinprod"),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("mononoke: fix bug")),
                SUMMARY => to_lua_string(s("fix")),
                TEST_PLAN => to_lua_string(s("testinprod")),
            },
        );

        // multiline title
        check_parse_commit(
            "mononoke: fix bug\nsecondline\nSummary: fix\nTest Plan: testinprod",
            PhabricatorMessage {
                title: ss("mononoke: fix bug\nsecondline"),
                summary: ss("fix"),
                test_plan: ss("testinprod"),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("mononoke: fix bug\nsecondline")),
                SUMMARY => to_lua_string(s("fix")),
                TEST_PLAN => to_lua_string(s("testinprod")),
            },
        );

        check_parse_commit(
            "Summary: fix\nTest Plan: testinprod",
            PhabricatorMessage {
                title: ss(""),
                summary: ss("fix"),
                test_plan: ss("testinprod"),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("")),
                SUMMARY => to_lua_string(s("fix")),
                TEST_PLAN => to_lua_string(s("testinprod")),
            },
        );

        // Tag should start at beginning of the line
        check_parse_commit(
            "Summary: fix\n Test Plan: testinprod",
            PhabricatorMessage {
                title: ss(""),
                summary: ss("fix\n Test Plan: testinprod"),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("")),
                SUMMARY => to_lua_string(s("fix\n Test Plan: testinprod")),
            },
        );

        check_parse_commit(
            "Summary: fix\nnot a tag: testinprod",
            PhabricatorMessage {
                title: ss(""),
                summary: ss("fix\nnot a tag: testinprod"),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("")),
                SUMMARY => to_lua_string(s("fix\nnot a tag: testinprod")),
            },
        );

        check_parse_commit(
            "Summary: fix\nFixed\na\nbug",
            PhabricatorMessage {
                title: ss(""),
                summary: ss("fix\nFixed\na\nbug"),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("")),
                SUMMARY => to_lua_string(s("fix\nFixed\na\nbug")),
            },
        );

        check_parse_commit(
            "Summary: fix\nCC:",
            PhabricatorMessage {
                title: ss(""),
                summary: ss("fix"),
                cc: Some(vec![]),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("")),
                SUMMARY => to_lua_string(s("fix")),
                CC => to_lua_array(vec![]),
            },
        );

        check_parse_commit(
            "CC: user1, user2, user3",
            PhabricatorMessage {
                title: ss(""),
                cc: Some(vec![s("user1"), s("user2"), s("user3")]),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("")),
                CC => to_lua_array(vec![s("user1"), s("user2"), s("user3")]),
            },
        );

        check_parse_commit(
            "Tasks: T1111, T2222, T3333",
            PhabricatorMessage {
                title: ss(""),
                tasks: Some(vec![s("T1111"), s("T2222"), s("T3333")]),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("")),
                TASKS => to_lua_array(vec![s("T1111"), s("T2222"), s("T3333")]),
            },
        );

        check_parse_commit(
            "Summary: fix\nTest Plan: testinprod\n\nReviewed By: stash, luk, simonfar",
            PhabricatorMessage {
                title: ss(""),
                summary: ss("fix"),
                test_plan: ss("testinprod"),
                reviewed_by: Some(vec![s("stash"), s("luk"), s("simonfar")]),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("")),
                SUMMARY => to_lua_string(s("fix")),
                TEST_PLAN => to_lua_string(s("testinprod")),
                REVIEWED_BY => to_lua_array(vec![s("stash"), s("luk"), s("simonfar")]),
            },
        );

        check_parse_commit(
            "mononoke: fix fixovich
Summary:

fix
of a mononoke
bug

Test Plan: testinprod
Reviewed By: stash
Reviewers: #mononoke,
CC: jsgf
Tasks: T1234
Differential Revision: https://url/D123
",
            PhabricatorMessage {
                title: ss("mononoke: fix fixovich"),
                summary: ss("fix\nof a mononoke\nbug"),
                test_plan: ss("testinprod"),
                reviewed_by: Some(vec![s("stash")]),
                reviewers: Some(vec![s("#mononoke")]),
                cc: Some(vec![s("jsgf")]),
                tasks: Some(vec![s("T1234")]),
                differential_revision: ss("https://url/D123"),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("mononoke: fix fixovich")),
                SUMMARY => to_lua_string(s("fix\nof a mononoke\nbug")),
                TEST_PLAN => to_lua_string(s("testinprod")),
                REVIEWED_BY => to_lua_array(vec![s("stash")]),
                REVIEWERS => to_lua_array(vec![s("#mononoke")]),
                CC => to_lua_array(vec![s("jsgf")]),
                TASKS => to_lua_array(vec![s("T1234")]),
                DIFFERENTIAL_REVISION =>  to_lua_string(s("https://url/D123")),
            },
        );

        // Parse (almost) a real commit message
        check_parse_commit(
            "mononoke: log error only once

Summary:
Previously `log_with_msg()` was logged twice if msg wasn't None - with and
without the message. This diff fixes it.

#accept2ship
Test Plan: buck check

Reviewers: simonfar, #mononoke

Reviewed By: simonfar

Subscribers: jsgf

Differential Revision: https://phabricator.intern.facebook.com/D1111111

Signature: 111111111:1111111111:bbbbbbbbbbbbbbbb",
            PhabricatorMessage {
                title: ss("mononoke: log error only once"),
                summary: ss(
                    "Previously `log_with_msg()` was logged twice if msg wasn't None - with and\n\
                     without the message. This diff fixes it.\n\
                     \n\
                     #accept2ship",
                ),
                test_plan: ss("buck check"),
                reviewed_by: Some(vec![s("simonfar")]),
                reviewers: Some(vec![s("simonfar"), s("#mononoke")]),
                subscribers: Some(vec![s("jsgf")]),
                differential_revision: ss("https://phabricator.intern.facebook.com/D1111111"),
                signature: ss("111111111:1111111111:bbbbbbbbbbbbbbbb"),
                ..Default::default()
            },
            hashmap! {
                TITLE => to_lua_string(s("mononoke: log error only once")),
                SUMMARY => to_lua_string(s("Previously `log_with_msg()` was logged twice if msg wasn't None - with and\n\
            without the message. This diff fixes it.\n\
            \n\
            #accept2ship")),
                TEST_PLAN => to_lua_string(s("buck check")),
                REVIEWED_BY => to_lua_array(vec![s("simonfar")]),
                REVIEWERS => to_lua_array(vec![s("simonfar"), s("#mononoke")]),
                SUBSCRIBERS => to_lua_array(vec![s("jsgf")]),
                DIFFERENTIAL_REVISION =>  to_lua_string(s("https://phabricator.intern.facebook.com/D1111111")),
                SIGNATURE =>  to_lua_string(s("111111111:1111111111:bbbbbbbbbbbbbbbb")),
            },
        );
    }
}
