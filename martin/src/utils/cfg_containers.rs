use std::vec::IntoIter;

use serde::{Deserialize, Serialize};

/// A serde helper to store a boolean as an object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptBoolObj<T> {
    #[default]
    #[serde(skip)]
    NoValue,
    Bool(bool),
    Object(T),
}

impl<T> OptBoolObj<T> {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::NoValue)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptOneMany<T> {
    #[default]
    NoVals,
    One(T),
    Many(Vec<T>),
}

impl<T> IntoIterator for OptOneMany<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::NoVals => Vec::new().into_iter(),
            Self::One(v) => vec![v].into_iter(),
            Self::Many(v) => v.into_iter(),
        }
    }
}

impl<T> OptOneMany<T> {
    pub fn new<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut iter = iter.into_iter();
        match (iter.next(), iter.next()) {
            (Some(first), Some(second)) => {
                let mut vec = Vec::with_capacity(iter.size_hint().0 + 2);
                vec.push(first);
                vec.push(second);
                vec.extend(iter);
                Self::Many(vec)
            }
            (Some(first), None) => Self::One(first),
            (None, _) => Self::NoVals,
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::NoVals)
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::NoVals => true,
            Self::One(_) => false,
            Self::Many(v) => v.is_empty(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        match self {
            Self::NoVals => [].iter(),
            Self::One(v) => std::slice::from_ref(v).iter(),
            Self::Many(v) => v.iter(),
        }
    }

    pub fn opt_iter(&self) -> Option<impl Iterator<Item = &T>> {
        match self {
            Self::NoVals => None,
            Self::One(v) => Some(std::slice::from_ref(v).iter()),
            Self::Many(v) => Some(v.iter()),
        }
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        match self {
            Self::NoVals => [].iter_mut(),
            Self::One(v) => std::slice::from_mut(v).iter_mut(),
            Self::Many(v) => v.iter_mut(),
        }
    }

    pub fn as_slice(&self) -> &[T] {
        match self {
            Self::NoVals => &[],
            Self::One(item) => std::slice::from_ref(item),
            Self::Many(v) => v.as_slice(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OptOneMany::{Many, NoVals, One};

    #[test]
    fn test_one_or_many() {
        let mut noval: OptOneMany<i32> = NoVals;
        let mut one = One(1);
        let mut many = Many(vec![1, 2, 3]);

        assert_eq!(OptOneMany::new(vec![1, 2, 3]), Many(vec![1, 2, 3]));
        assert_eq!(OptOneMany::new(vec![1]), One(1));
        assert_eq!(OptOneMany::new(Vec::<i32>::new()), NoVals);

        assert_eq!(noval.iter_mut().collect::<Vec<_>>(), Vec::<&i32>::new());
        assert_eq!(one.iter_mut().collect::<Vec<_>>(), vec![&1]);
        assert_eq!(many.iter_mut().collect::<Vec<_>>(), vec![&1, &2, &3]);

        assert_eq!(noval.iter().collect::<Vec<_>>(), Vec::<&i32>::new());
        assert_eq!(one.iter().collect::<Vec<_>>(), vec![&1]);
        assert_eq!(many.iter().collect::<Vec<_>>(), vec![&1, &2, &3]);

        assert_eq!(noval.opt_iter().map(Iterator::collect::<Vec<_>>), None);
        assert_eq!(one.opt_iter().map(Iterator::collect), Some(vec![&1]));
        assert_eq!(
            many.opt_iter().map(Iterator::collect),
            Some(vec![&1, &2, &3])
        );

        assert_eq!(noval.as_slice(), Vec::<i32>::new().as_slice());
        assert_eq!(one.as_slice(), &[1]);
        assert_eq!(many.as_slice(), &[1, 2, 3]);

        assert_eq!(noval.into_iter().collect::<Vec<_>>(), Vec::<i32>::new());
        assert_eq!(one.into_iter().collect::<Vec<_>>(), vec![1]);
        assert_eq!(many.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
    }
}
