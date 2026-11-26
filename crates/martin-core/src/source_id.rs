//! Interned names of tile sources, fonts and sprites, and server-defined sets of them.
//!
//! An id is minted once per distinct name, and never freed.
//! Only code that registers a source mints ids: a request resolves its names through
//! the registry, so untrusted input can never grow the table.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};
use std::num::NonZeroU32;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use arc_swap::ArcSwap;

/// A tile source, font or sprite name, or a server-defined set of them, as one `u32`.
///
/// A name is the set of only itself, so a single source and an alias of several share one id space.
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceId(NonZeroU32);

/// What one id stands for.
enum Entry {
    /// A single name, with itself as its only member.
    Name(&'static str, [SourceId; 1]),
    /// A server-defined list of names, in order.
    Set(&'static [SourceId]),
}

/// Segment `s` holds `2^s` entries, so 32 segments cover every `u32` id without moving any.
const SEGMENTS: usize = 32;

/// The writer's indexes, only touched while interning.
#[derive(Default)]
struct Writer {
    /// Ids handed out so far.
    len: u32,
    /// The id of each interned name.
    names: HashMap<&'static str, SourceId>,
    /// The id of each interned set, also published lock-free in [`Table::sets`].
    sets: HashMap<&'static [SourceId], SourceId>,
}

/// The process-wide table behind every [`SourceId`].
struct Table {
    /// Entries by id, written once and read without locks.
    segments: [OnceLock<Box<[OnceLock<Entry>]>>; SEGMENTS],
    /// Serialises interning.
    writer: Mutex<Writer>,
    /// A read-only copy of [`Writer::sets`], swapped on every new set.
    sets: ArcSwap<HashMap<&'static [SourceId], SourceId>>,
}

/// The table, created on first use.
static TABLE: LazyLock<Table> = LazyLock::new(|| Table {
    segments: [const { OnceLock::new() }; SEGMENTS],
    writer: Mutex::default(),
    sets: ArcSwap::default(),
});

/// The segment and the offset inside it that hold `id`.
fn slot(id: SourceId) -> (usize, usize) {
    let index = id.0.get();
    let segment = index.ilog2();
    (segment as usize, (index - (1 << segment)) as usize)
}

impl Table {
    /// The entry behind an id this table handed out.
    fn entry(&self, id: SourceId) -> &Entry {
        let (segment, offset) = slot(id);
        self.segments[segment]
            .get()
            .and_then(|s| s[offset].get())
            .expect("a SourceId is only created together with its entry")
    }

    /// Stores `entry` under the next id.
    fn push(&self, writer: &mut Writer, entry: impl FnOnce(SourceId) -> Entry) -> SourceId {
        writer.len += 1;
        let id = SourceId(NonZeroU32::new(writer.len).expect("the id counter starts at one"));
        let (segment, offset) = slot(id);
        let slots = self.segments[segment].get_or_init(|| (0..1_usize << segment).map(|_| OnceLock::new()).collect());
        assert!(slots[offset].set(entry(id)).is_ok(), "each id is pushed once");
        id
    }

    /// Locks the writer; a panic while interning leaves the indexes consistent, so poisoning is ignored.
    fn writer(&self) -> std::sync::MutexGuard<'_, Writer> {
        self.writer.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl SourceId {
    /// The id of `name`, minting one on first use.
    ///
    /// Call this only while registering a source, never with names from a request:
    /// ids are never freed, so request input would grow the table without bound.
    #[must_use]
    pub fn intern(name: &str) -> Self {
        let table = &*TABLE;
        let mut writer = table.writer();
        if let Some(&id) = writer.names.get(name) {
            return id;
        }
        let name: &'static str = Box::leak(name.into());
        let id = table.push(&mut writer, |id| Entry::Name(name, [id]));
        writer.names.insert(name, id);
        id
    }

    /// The id of the server-defined list `members`, minting one on first use.
    ///
    /// A single member is its own set. Order matters: `[a, b]` and `[b, a]` are different sets.
    /// A member that is itself a set stands for its names, so `[ab, c]` is `[a, b, c]`.
    /// Like [`Self::intern`], call this only while registering, e.g. an alias.
    ///
    /// # Panics
    ///
    /// If `members` is empty.
    #[must_use]
    pub fn intern_set(members: &[Self]) -> Self {
        assert!(!members.is_empty(), "a set has at least one member");
        if let [one] = members {
            return *one;
        }
        let members = Self::flatten(members);
        let table = &*TABLE;
        let mut writer = table.writer();
        if let Some(&id) = writer.sets.get(&*members) {
            return id;
        }
        let members: &'static [Self] = Box::leak(members.into());
        let id = table.push(&mut writer, |_| Entry::Set(members));
        writer.sets.insert(members, id);
        table.sets.store(Arc::new(writer.sets.clone()));
        id
    }

    /// The id of an already interned `name`, without minting one.
    #[must_use]
    pub fn lookup(name: &str) -> Option<Self> {
        TABLE.writer().names.get(name).copied()
    }

    /// The id of an already interned list, without minting one or taking a lock.
    ///
    /// A single member is its own set, and a member that is a set stands for its names, as in [`Self::intern_set`].
    #[must_use]
    pub fn find_set(members: &[Self]) -> Option<Self> {
        match members {
            [] => None,
            [one] => Some(*one),
            _ => TABLE.sets.load().get(&*Self::flatten(members)).copied(),
        }
    }

    /// `members` with every set replaced by its names, borrowed when it lists only names.
    fn flatten(members: &[Self]) -> Cow<'_, [Self]> {
        if members.iter().all(|member| member.name().is_some()) {
            Cow::Borrowed(members)
        } else {
            Cow::Owned(members.iter().flat_map(|member| member.members()).copied().collect())
        }
    }

    /// The name, or `None` for a set of several names.
    #[must_use]
    pub fn name(self) -> Option<&'static str> {
        match TABLE.entry(self) {
            Entry::Name(name, _) => Some(name),
            Entry::Set(_) => None,
        }
    }

    /// The names this id stands for, in order; a name is the only member of itself.
    #[must_use]
    pub fn members(self) -> &'static [Self] {
        match TABLE.entry(self) {
            Entry::Name(_, me) => me,
            Entry::Set(members) => members,
        }
    }

    /// Whether `member` is this id or one of its members.
    #[must_use]
    pub fn contains(self, member: Self) -> bool {
        self == member || self.members().contains(&member)
    }
}

impl Display for SourceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut members = self.members().iter();
        if let Some(first) = members.next() {
            f.write_str(first.name().expect("set members are names"))?;
        }
        for member in members {
            f.write_str(",")?;
            f.write_str(member.name().expect("set members are names"))?;
        }
        Ok(())
    }
}

impl Debug for SourceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "SourceId({:?})", self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_a_name_twice_returns_the_same_id() {
        assert_eq!(
            SourceId::intern("source_id::twice"),
            SourceId::intern("source_id::twice")
        );
    }

    #[test]
    fn lookup_of_an_unknown_name_mints_nothing() {
        assert_eq!(SourceId::lookup("source_id::never-registered"), None);
        assert_eq!(SourceId::lookup("source_id::never-registered"), None);
    }

    #[test]
    fn a_name_is_its_own_single_member_set() {
        let a = SourceId::intern("source_id::single");
        assert_eq!(SourceId::intern_set(&[a]), a);
        assert_eq!(SourceId::find_set(&[a]), Some(a));
        assert_eq!(a.members(), &[a]);
        assert_eq!(a.name(), Some("source_id::single"));
    }

    #[test]
    fn a_set_is_found_by_its_members_in_order() {
        let a = SourceId::intern("source_id::set-a");
        let b = SourceId::intern("source_id::set-b");
        let ab = SourceId::intern_set(&[a, b]);
        assert_eq!(SourceId::find_set(&[a, b]), Some(ab));
        assert_eq!(SourceId::find_set(&[b, a]), None);
        assert_eq!(ab.members(), &[a, b]);
        assert_eq!(ab.name(), None);
        assert!(ab.contains(a));
        assert!(!a.contains(ab));
    }

    #[test]
    fn find_set_of_an_unregistered_list_mints_nothing() {
        let a = SourceId::intern("source_id::unregistered-a");
        let b = SourceId::intern("source_id::unregistered-b");
        assert_eq!(SourceId::find_set(&[a, b]), None);
        assert_eq!(SourceId::find_set(&[a, b]), None);
    }

    #[test]
    fn a_set_of_sets_is_the_set_of_their_names() {
        let a = SourceId::intern("source_id::nested-a");
        let b = SourceId::intern("source_id::nested-b");
        let c = SourceId::intern("source_id::nested-c");
        let ab = SourceId::intern_set(&[a, b]);

        let abc = SourceId::intern_set(&[ab, c]);

        assert_eq!(abc, SourceId::intern_set(&[a, b, c]));
        assert_eq!(SourceId::find_set(&[ab, c]), Some(abc));
        assert_eq!(
            abc.to_string(),
            "source_id::nested-a,source_id::nested-b,source_id::nested-c"
        );
    }

    #[test]
    fn display_and_debug_show_names() {
        let a = SourceId::intern("source_id::shown-a");
        let b = SourceId::intern("source_id::shown-b");
        let ab = SourceId::intern_set(&[a, b]);
        assert_eq!(ab.to_string(), "source_id::shown-a,source_id::shown-b");
        assert_eq!(format!("{a:?}"), r#"SourceId("source_id::shown-a")"#);
    }

    #[test]
    fn ids_survive_crossing_segment_boundaries() {
        let ids: Vec<(String, SourceId)> = (0..300)
            .map(|i| {
                let name = format!("source_id::many-{i}");
                let id = SourceId::intern(&name);
                (name, id)
            })
            .collect();
        for (name, id) in ids {
            assert_eq!(id.name(), Some(name.as_str()));
        }
    }
}
