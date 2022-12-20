use serde::{Deserialize, Serialize};
use std::mem;
use std::slice::Iter;
use std::vec::IntoIter;

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
            // OneOrMany::One(s) => OneOrManyIter::One(Some(s)),
            // OneOrMany::Many(v) => OneOrManyIter::Many(v.into_iter()),
            OneOrMany::One(v) => vec![v].into_iter(),
            OneOrMany::Many(v) => v.into_iter(),
        }
    }
}

impl<T: Clone> OneOrMany<T> {
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        match self {
            OneOrMany::Many(v) => v.iter_mut(),
            OneOrMany::One(v) => std::slice::from_mut(v).iter_mut(),
        }
    }

    pub fn iter(&self) -> Iter<T> {
        self.as_slice().iter()
    }

    pub fn as_slice(&self) -> &[T] {
        match self {
            OneOrMany::One(item) => std::slice::from_ref(item),
            OneOrMany::Many(v) => v.as_slice(),
        }
    }

    pub fn map<R: Clone, F>(self, mut f: F) -> crate::Result<OneOrMany<R>>
    where
        F: FnMut(T) -> crate::Result<R>,
    {
        Ok(match self {
            Self::One(v) => OneOrMany::One(f(v)?),
            Self::Many(v) => OneOrMany::Many(v.into_iter().map(f).collect::<crate::Result<_>>()?),
        })
    }

    pub fn generalize(self) -> Vec<T> {
        match self {
            Self::One(v) => vec![v],
            Self::Many(v) => v,
        }
    }

    pub fn merge(&mut self, other: Self) {
        // There is no allocation with Vec::new()
        *self = match (mem::replace(self, Self::Many(Vec::new())), other) {
            (Self::One(a), Self::One(b)) => Self::Many(vec![a, b]),
            (Self::One(a), Self::Many(mut b)) => {
                b.insert(0, a);
                Self::Many(b)
            }
            (Self::Many(mut a), Self::One(b)) => {
                a.push(b);
                Self::Many(a)
            }
            (Self::Many(mut a), Self::Many(b)) => {
                a.extend(b);
                Self::Many(a)
            }
        };
    }
}
