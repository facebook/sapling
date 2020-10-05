/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// In case iterator has 0 or 2+ items, fails with errors
/// produced by corresponding error factories
pub fn get_only_item<T, E, N, NE, M, ME>(
    items: impl IntoIterator<Item = T>,
    no_items_error: N,
    many_items_error: M,
) -> Result<T, E>
where
    N: FnOnce() -> NE,
    NE: Into<E>,
    M: FnOnce(T, T) -> ME,
    ME: Into<E>,
{
    let mut iter = items.into_iter();
    let maybe_first = iter.next();
    let maybe_second = iter.next();
    match (maybe_first, maybe_second) {
        (None, None) => Err(no_items_error().into()),
        (Some(only_item), None) => Ok(only_item),
        (Some(item1), Some(item2)) => Err(many_items_error(item1, item2).into()),
        (None, Some(_)) => panic!("iterator returns Some after None"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{anyhow, Error};

    #[test]
    fn test_success() {
        let v: Vec<u8> = vec![1];
        let r: Result<_, Error> =
            get_only_item(v, || anyhow!("no items"), |_, _| anyhow!("many items"));
        assert_eq!(r.unwrap(), 1);
    }

    #[test]
    fn test_no_items() {
        let v: Vec<u8> = vec![];
        let res: Result<_, Error> =
            get_only_item(v, || anyhow!("no items"), |_, _| anyhow!("many items"));
        assert!(res.is_err());
        assert_eq!(format!("{}", res.unwrap_err()), "no items".to_string());
    }

    #[test]
    fn test_too_many_items() {
        let v: Vec<u8> = vec![1, 2, 3];
        let res: Result<_, Error> =
            get_only_item(v, || anyhow!("no items"), |_, _| anyhow!("many items"));
        assert!(res.is_err());
        assert_eq!(format!("{}", res.unwrap_err()), "many items".to_string());
    }

    #[test]
    fn test_too_many_items_args() {
        let v: Vec<u8> = vec![1, 2, 3];
        let too_many_items = |i1, i2| {
            assert_eq!(i1, 1);
            assert_eq!(i2, 2);
            anyhow!("many items")
        };
        let res: Result<_, Error> = get_only_item(v, || anyhow!("no items"), too_many_items);
        assert!(res.is_err());
    }
}
