use std::vec::IntoIter;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> IntoIterator for OneOrMany<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::One(v) => vec![v].into_iter(),
            Self::Many(v) => v.into_iter(),
        }
    }
}

impl<T: Clone> OneOrMany<T> {
    pub fn new_opt<I: IntoIterator<Item = T>>(iter: I) -> Option<Self> {
        let mut iter = iter.into_iter();
        match (iter.next(), iter.next()) {
            (Some(first), Some(second)) => {
                let mut vec = Vec::with_capacity(iter.size_hint().0 + 2);
                vec.push(first);
                vec.push(second);
                vec.extend(iter);
                Some(Self::Many(vec))
            }
            (Some(first), None) => Some(Self::One(first)),
            (None, _) => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::One(_) => false,
            Self::Many(v) => v.is_empty(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        match self {
            OneOrMany::Many(v) => v.iter(),
            OneOrMany::One(v) => std::slice::from_ref(v).iter(),
        }
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        match self {
            Self::Many(v) => v.iter_mut(),
            Self::One(v) => std::slice::from_mut(v).iter_mut(),
        }
    }

    pub fn as_slice(&self) -> &[T] {
        match self {
            Self::One(item) => std::slice::from_ref(item),
            Self::Many(v) => v.as_slice(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OneOrMany::{Many, One};

    #[test]
    fn test_one_or_many() {
        let mut one = One(1);
        let mut many = Many(vec![1, 2, 3]);

        assert_eq!(OneOrMany::new_opt(vec![1, 2, 3]), Some(Many(vec![1, 2, 3])));
        assert_eq!(OneOrMany::new_opt(vec![1]), Some(One(1)));
        assert_eq!(OneOrMany::new_opt(Vec::<i32>::new()), None);

        assert_eq!(one.iter_mut().collect::<Vec<_>>(), vec![&1]);
        assert_eq!(many.iter_mut().collect::<Vec<_>>(), vec![&1, &2, &3]);

        assert_eq!(one.iter().collect::<Vec<_>>(), vec![&1]);
        assert_eq!(many.iter().collect::<Vec<_>>(), vec![&1, &2, &3]);

        assert_eq!(one.as_slice(), &[1]);
        assert_eq!(many.as_slice(), &[1, 2, 3]);

        assert_eq!(one.into_iter().collect::<Vec<_>>(), vec![1]);
        assert_eq!(many.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
    }
}
