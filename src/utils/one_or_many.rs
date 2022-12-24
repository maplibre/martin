use serde::{Deserialize, Serialize};
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_one_or_many() {
        let mut one = OneOrMany::One(1);
        let mut many = OneOrMany::Many(vec![1, 2, 3]);

        assert_eq!(one.iter().collect::<Vec<_>>(), vec![&1]);
        assert_eq!(many.iter().collect::<Vec<_>>(), vec![&1, &2, &3]);

        assert_eq!(one.iter_mut().collect::<Vec<_>>(), vec![&1]);
        assert_eq!(many.iter_mut().collect::<Vec<_>>(), vec![&1, &2, &3]);

        assert_eq!(one.as_slice(), &[1]);
        assert_eq!(many.as_slice(), &[1, 2, 3]);

        assert_eq!(one.into_iter().collect::<Vec<_>>(), vec![1]);
        assert_eq!(many.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
    }
}
