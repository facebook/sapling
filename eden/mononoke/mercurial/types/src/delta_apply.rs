/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Result;
use bytes::Bytes;
use heapsize_derive::HeapSizeOf;
use std::cmp;

use crate::delta::{Delta, Fragment};

/*
* Algorithm is taken from fbcode/scm/hg/mercurial/mpatch.c
*/

/// Wrap all Fragments and return FragmentWrapperIterator.
/// Gather all contents hold fragments contents in one vector.
pub fn wrap_deltas<I: IntoIterator<Item = Delta>>(
    deltas: I,
) -> (Vec<FragmentWrapperIterator>, Bytes) {
    let mut wrapped_deltas = Vec::new();
    let mut data = Bytes::new();
    let mut content_offset = 0;

    for delta in deltas {
        let wrapped_delta = FragmentWrapperIterator::new(&delta, content_offset as i64);
        for frag in delta.fragments() {
            data.extend_from_slice(frag.content.as_slice());
            content_offset += frag.content.len();
        }

        wrapped_deltas.push(wrapped_delta);
    }

    (wrapped_deltas, data)
}

// Fragment Wrapper, it does not have actual data, only references to real data
#[derive(Clone, Eq, Debug, PartialEq, Ord, PartialOrd, HeapSizeOf)]
pub struct FragmentWrapper {
    pub start: i64,
    pub end: i64,
    pub len: i64,
    pub content_start: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, HeapSizeOf, Default)]
pub struct FragmentWrapperIterator {
    // Struct for holding Fragments and updating current head
    frags: Vec<FragmentWrapper>,
    cur_pointer: usize,
}

impl FragmentWrapperIterator {
    pub fn new(delta: &Delta, content_offset: i64) -> FragmentWrapperIterator {
        // Convert Delta to Vec<FragmentWrapper>, using global offset of the content in Content Bytes
        let mut frag_wrappers = Vec::new();
        let mut offset = content_offset;

        for frag in delta.fragments() {
            let frag_wrapper = FragmentWrapper {
                start: frag.start as i64,
                end: frag.end as i64,
                len: frag.content_length() as i64,
                content_start: offset as i64,
            };
            offset += frag.content_length() as i64;
            frag_wrappers.push(frag_wrapper);
        }

        FragmentWrapperIterator {
            frags: frag_wrappers,
            cur_pointer: 0,
        }
    }

    pub fn content_length(&self) -> i64 {
        let mut size = 0;
        for frag in self.frags.as_slice() {
            size += frag.len;
        }
        size
    }

    pub fn current_fragment_mut(&mut self) -> &mut FragmentWrapper {
        &mut self.frags[self.cur_pointer]
    }

    pub fn current_fragment(&self) -> &FragmentWrapper {
        &self.frags[self.cur_pointer]
    }

    pub fn end(&self) -> bool {
        self.cur_pointer == self.frags.len()
    }

    pub fn go_next(&mut self) {
        self.cur_pointer += 1;
    }

    pub fn set_start(&mut self) {
        self.cur_pointer = 0;
    }

    pub fn fragments(&self) -> &[FragmentWrapper] {
        self.frags.as_slice()
    }

    pub fn push(&mut self, frag: FragmentWrapper) {
        self.frags.push(frag);
    }

    pub fn into_delta(&self, data: Bytes) -> Result<Delta> {
        let mut frags = Vec::new();

        for frag_wrapper in self.frags.as_slice() {
            let content_start = frag_wrapper.content_start as usize;
            let content_end = (frag_wrapper.content_start + frag_wrapper.len) as usize;

            let frag = Fragment {
                start: frag_wrapper.start as usize,
                end: frag_wrapper.end as usize,
                content: data.slice(content_start, content_end).to_vec(),
            };
            frags.push(frag);
        }
        Delta::new(frags)
    }
}

/// Merge 2 sequential deltas into 1 delta
fn combine(
    a: &mut FragmentWrapperIterator,
    b: &mut FragmentWrapperIterator,
) -> FragmentWrapperIterator {
    let mut combined: FragmentWrapperIterator = Default::default();
    let mut offset = 0;
    let mut post;

    a.set_start();
    for b_frag in b.fragments() {
        offset = gather(&mut combined, a, b_frag.start, offset);

        post = discard(a, b_frag.end, offset);

        let frag = FragmentWrapper {
            start: b_frag.start - offset,
            end: b_frag.end - post,
            len: b_frag.len,
            content_start: b_frag.content_start,
        };
        combined.push(frag);
        offset = post;
    }

    // process tail
    while !a.end() {
        combined.push(a.current_fragment().clone());
        a.go_next();
    }
    combined
}

/// Copy all fragments from src to dst until cut
fn gather(
    dst: &mut FragmentWrapperIterator,
    src: &mut FragmentWrapperIterator,
    cut: i64,
    mut offset: i64,
) -> i64 {
    while !src.end() {
        let frag = src.current_fragment().clone();

        if frag.start + offset >= cut {
            break;
        }

        let postend = offset + frag.start + frag.len;
        if postend <= cut {
            offset += frag.start + frag.len - frag.end;
            dst.push(frag.clone());

            src.go_next();
        } else {
            let new_start = cmp::min(cut - offset, frag.end);
            let prev_len = cmp::min(cut - offset - frag.start, frag.len);

            offset += frag.start + prev_len - new_start;

            let prev_content_start = frag.content_start;
            let new_content_start = frag.content_start + prev_len;

            let new_frag = FragmentWrapper {
                start: frag.start,
                end: new_start,
                len: prev_len,
                content_start: prev_content_start,
            };

            dst.push(new_frag);

            let frag_mut = src.current_fragment_mut();

            frag_mut.start = new_start;
            frag_mut.len = frag.len - prev_len;
            frag_mut.content_start = new_content_start;
            break;
        }
    }
    offset
}

/// Delete all fragments from src until cut
fn discard(src: &mut FragmentWrapperIterator, cut: i64, mut offset: i64) -> i64 {
    while !src.end() {
        let frag = src.current_fragment().clone();

        if frag.start + offset >= cut {
            break;
        }

        let postend = offset + frag.start + frag.len;
        if postend <= cut {
            offset += frag.start + frag.len - frag.end;
            src.go_next();
        } else {
            let new_start = cmp::min(cut - offset, frag.end);
            let prev_len = cmp::min(cut - offset - frag.start, frag.len);

            offset += frag.start + prev_len - new_start;

            let new_content_start = frag.content_start + prev_len;

            let frag_mut = src.current_fragment_mut();

            frag_mut.start = new_start;
            frag_mut.len = frag.len - prev_len;
            frag_mut.content_start = new_content_start;
            break;
        }
    }
    offset
}

/// Fold deltas in the range [start, end)
pub fn mpatch_fold(
    deltas: &Vec<FragmentWrapperIterator>,
    start: usize,
    end: usize,
) -> FragmentWrapperIterator {
    assert!(start < end);

    if start + 1 == end {
        deltas[start].clone()
    } else {
        let half_deltas_cnt = (end - start) / 2;
        combine(
            &mut mpatch_fold(deltas, start, start + half_deltas_cnt),
            &mut mpatch_fold(deltas, start + half_deltas_cnt, end),
        )
    }
}
