/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Mutex;

use futures::future::BoxFuture;
use futures::TryStreamExt;

use crate::errors::programming;
use crate::Result;
use crate::Set;
use crate::Vertex;

/// Pre-process a parent function that might have cycles.
/// Return a new parent function that won't have cycles.
///
/// This function is not fast. Only use it on small graphs.
pub fn break_parent_func_cycle<F>(parent_func: F) -> impl Fn(Vertex) -> Result<Vec<Vertex>>
where
    F: Fn(Vertex) -> Result<Vec<Vertex>>,
{
    #[derive(Default)]
    struct State {
        /// Previously calculated parents.
        known: HashMap<Vertex, Vec<Vertex>>,
    }
    impl State {
        fn is_ancestor(&self, ancestor: &Vertex, descentant: &Vertex) -> bool {
            if ancestor == descentant {
                return true;
            }
            let mut to_visit = vec![descentant];
            let mut visited = HashSet::new();
            while let Some(v) = to_visit.pop() {
                if !visited.insert(v) {
                    continue;
                }
                if let Some(parents) = self.known.get(&v) {
                    for p in parents {
                        if p == ancestor {
                            return true;
                        }
                        to_visit.push(p);
                    }
                }
            }
            false
        }
    }
    let state: Mutex<State> = Default::default();

    move |v: Vertex| -> Result<Vec<Vertex>> {
        let mut state = state.lock().unwrap();
        if let Some(parents) = state.known.get(&v) {
            return Ok(parents.clone());
        }
        let parents = parent_func(v.clone())?;
        let mut result = Vec::with_capacity(parents.len());
        for p in parents {
            if !state.is_ancestor(&v, &p) {
                // Not a cycle.
                result.push(p);
            }
        }
        state.known.insert(v, result.clone());
        Ok(result)
    }
}

/// Given a `set` (sub-graph) and a filter function that selects "known"
/// subset of its input, apply filter to `set`.
///
/// The filter funtion must have following properties:
/// - filter(xs) + filter(ys) = filter(xs + ys)
/// - If its input contains both X and Y and X is an ancestor of Y in the
///   sub-graph, and its output contains Y, then its output must also
///   contain Y's ancestor X.
///   In other words, if vertex X is considered known, then ancestors
///   of X are also known.
///
/// This function has a similar signature with `filter`, but it utilizes
/// the above properties to test (much) less vertexes for a large input
/// set.
///
/// The idea of the algorithm comes from Mercurial's `setdiscovery.py`,
/// introduced by [1]. `setdiscovery.py` is used to figure out what
/// commits are needed to be pushed or pulled.
///
/// [1]: https://www.mercurial-scm.org/repo/hg/rev/cb98fed52495
pub async fn filter_known<'a>(
    set: Set,
    filter_known_func: &(
         dyn (Fn(&[Vertex]) -> BoxFuture<'a, Result<Vec<Vertex>>>) + Send + Sync + 'a
     ),
) -> Result<Set> {
    // Figure out unassigned (missing) vertexes that do need to be inserted.
    //
    // remaining:  subset not categorized.
    // known:      subset categorized as "known"
    // unknown:    subset categorized as "unknown"
    //
    // See [1] for the algorithm, basically:
    // - Take a subset (sample) of "remaining".
    // - Check the subset (sample). Divide it into (new_known, new_unknown).
    // - known   |= ancestors(new_known)
    // - unknown |= descendants(new_unknown)
    // - remaining -= known | unknown
    // - Repeat until "remaining" becomes empty.
    let mut remaining = set;
    let subdag = match remaining.dag() {
        Some(dag) => dag,
        None => return programming("filter_known requires set to associate to a Dag"),
    };
    let mut known = Set::empty();

    for i in 1usize.. {
        let remaining_old_len = remaining.count().await?;
        if remaining_old_len == 0 {
            break;
        }

        // Sample: heads, roots, and the "middle point" from "remaining".
        let sample = if i <= 2 {
            // But for the first few queries, let's just check the roots.
            // This could reduce remote lookups, when we only need to
            // query the roots to rule out all `remaining` vertexes.
            subdag.roots(remaining.clone()).await?
        } else {
            subdag
                .roots(remaining.clone())
                .await?
                .union(&subdag.heads(remaining.clone()).await?)
                .union(&remaining.skip((remaining_old_len as u64) / 2).take(1))
        };
        let sample: Vec<Vertex> = sample.iter().await?.try_collect().await?;
        let new_known = filter_known_func(&sample).await?;
        let new_unknown: Vec<Vertex> = {
            let filtered_set: HashSet<Vertex> = new_known.iter().cloned().collect();
            sample
                .iter()
                .filter(|v| !filtered_set.contains(v))
                .cloned()
                .collect()
        };

        let new_known = Set::from_static_names(new_known);
        let new_unknown = Set::from_static_names(new_unknown);

        let new_known = subdag.ancestors(new_known).await?;
        let new_unknown = subdag.descendants(new_unknown).await?;

        remaining = remaining.difference(&new_known.union(&new_unknown));
        let remaining_new_len = remaining.count().await?;

        let known_old_len = known.count().await?;
        known = known.union(&new_known);
        let known_new_len = known.count().await?;

        tracing::trace!(
            target: "dag::utils::filter_known",
            "#{} remaining {} => {}, known: {} => {}",
            i,
            remaining_old_len,
            remaining_new_len,
            known_old_len,
            known_new_len
        );
    }

    Ok(known)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_break_parent_func_cycle() -> Result<()> {
        let parent_func = |n: Vertex| -> Result<Vec<Vertex>> { Ok(vec![n, v("1"), v("2")]) };
        let parent_func_no_cycle = break_parent_func_cycle(parent_func);
        assert_eq!(parent_func_no_cycle(v("A"))?, vec![v("1"), v("2")]);
        assert_eq!(parent_func_no_cycle(v("1"))?, vec![v("2")]);
        assert_eq!(parent_func_no_cycle(v("2"))?, vec![]);
        Ok(())
    }

    #[test]
    fn test_break_parent_func_cycle_linear() -> Result<()> {
        let parent_func = |n: Vertex| -> Result<Vec<Vertex>> {
            let list = "0123456789".chars().map(|c| v(c)).collect::<Vec<_>>();
            let parents = match list.iter().position(|x| x == &n) {
                Some(i) if i > 0 => vec![list[i - 1].clone()],
                _ => vec![],
            };
            Ok(parents)
        };
        let parent_func_no_cycle = break_parent_func_cycle(parent_func);
        assert_eq!(parent_func_no_cycle(v("2"))?, vec![v("1")]);
        assert_eq!(parent_func_no_cycle(v("9"))?, vec![v("8")]);
        assert_eq!(parent_func_no_cycle(v("8"))?, vec![v("7")]);
        assert_eq!(parent_func_no_cycle(v("1"))?, vec![v("0")]);
        assert_eq!(parent_func_no_cycle(v("5"))?, vec![v("4")]);
        assert_eq!(parent_func_no_cycle(v("6"))?, vec![v("5")]);
        assert_eq!(parent_func_no_cycle(v("4"))?, vec![v("3")]);
        assert_eq!(parent_func_no_cycle(v("0"))?, vec![]);
        assert_eq!(parent_func_no_cycle(v("3"))?, vec![v("2")]);
        assert_eq!(parent_func_no_cycle(v("7"))?, vec![v("6")]);
        Ok(())
    }

    /// Quickly create a Vertex.
    fn v(name: impl ToString) -> Vertex {
        Vertex::copy_from(name.to_string().as_bytes())
    }
}
