/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Mutex;

use crate::Result;
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
