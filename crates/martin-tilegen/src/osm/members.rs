//! The relations of a table's relation sets, and the nodes and ways that are their members, found by the pass over
//! the relations before the nodes and ways are read.

use std::collections::HashMap;

use cel::Value;
use osmflow::{Block, MemberType, Relation, TagKey};

use crate::expr::{ExprKeys, PropSlots};
use crate::plan::{ColumnDef, Condition, PlannedRelationSet, ValueKind};
use crate::props::{KeyId, Prop, SourceStr};
use crate::source::Membership;

/// A relation of a relation set, as its members see it.
struct SetRelation {
    /// The position of the set among the table's relation sets.
    set: u16,
    id: i64,
    /// The relation's value of each of the set's tags, by position.
    tags: Vec<Option<Box<str>>>,
}

/// One membership of a node or way in a relation of a set, as one block of relations gives it.
struct Found {
    kind: MemberKind,
    member: i64,
    set: u16,
    relation: i64,
    /// The role's position among the block's roles.
    role: u32,
}

/// The kinds of members that see their relations.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberKind {
    /// A node, read as a point.
    Node,
    /// A way, read as a line or an area.
    Way,
}

/// One membership: a member, the position of its relation, and the position of its role.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Entry {
    member: i64,
    relation: u32,
    role: u32,
}

/// The relation sets of a table, and how each set's `where` is checked on a block's relations.
pub(super) struct SetFilters<'p> {
    sets: Vec<SetFilter<'p>>,
}

/// One relation set and the tags its `where` reads.
struct SetFilter<'p> {
    set: &'p PlannedRelationSet,
    /// The tags the `where` reads, unless it reads them all.
    columns: Option<Vec<String>>,
    /// The key of each of `columns`, by position.
    keys: ExprKeys,
    /// The relations whose block strings can hold a match, unless the `where` cannot tell.
    condition: Option<Condition>,
}

impl<'p> SetFilters<'p> {
    /// The filters of `sets`.
    pub(super) fn new(sets: &'p [PlannedRelationSet]) -> Self {
        let sets = sets
            .iter()
            .map(|set| {
                let columns: Option<Vec<String>> =
                    (!set.filter.needs_all_keys()).then(|| set.filter.columns().map(str::to_owned).collect());
                let keys = columns.as_deref().map_or_default(ExprKeys::columns);
                let condition = columns.as_ref().and_then(|columns| {
                    let defs: Vec<ColumnDef> = columns
                        .iter()
                        .map(|name| ColumnDef::Value(name.clone(), Some(ValueKind::String)))
                        .collect();
                    set.filter.condition(&defs, &[]).ok()
                });
                SetFilter {
                    set,
                    columns,
                    keys,
                    condition,
                }
            })
            .collect();
        Self { sets }
    }

    /// The names of the sets, by position.
    pub(super) fn names(&self) -> Vec<String> {
        self.sets.iter().map(|filter| filter.set.def.name.clone()).collect()
    }

    /// Whether the table has no relation sets.
    pub(super) fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// The conditions on block strings that some relation of a set must meet, or `None` if a set cannot tell.
    pub(super) fn block_conditions(&self) -> Option<Vec<Condition>> {
        self.sets.iter().map(|filter| filter.condition.clone()).collect()
    }

    /// Adds to `found` the relations of `block` that some set holds, and their members.
    pub(super) fn read_block(&self, block: &Block, found: &mut FoundRelations) {
        let mut slots = PropSlots::default();
        let roles_start = found.roles.len();
        let mut roles: HashMap<&str, u32> = HashMap::new();
        for (set, filter) in (0u16..).zip(&self.sets) {
            let tag_keys: Vec<Option<TagKey<'_>>> = filter.set.def.tags.iter().map(|t| block.tag_key(t)).collect();
            let filter_keys: Vec<Option<TagKey<'_>>> = filter
                .columns
                .iter()
                .flatten()
                .map(|column| block.tag_key(column))
                .collect();
            for relation in block.relations() {
                if !filter.holds(&relation, &filter_keys, &mut slots) {
                    continue;
                }
                found.relations.push(SetRelation {
                    set,
                    id: relation.id(),
                    tags: tag_keys
                        .iter()
                        .map(|key| {
                            key.and_then(|key| relation.tag(key))
                                .filter(|v| !v.is_empty())
                                .map(Box::from)
                        })
                        .collect(),
                });
                for member in relation.members() {
                    let kind = match member.kind {
                        MemberType::Node => MemberKind::Node,
                        MemberType::Way => MemberKind::Way,
                        MemberType::Relation => continue,
                    };
                    let next = u32::try_from(roles_start + roles.len()).expect("fewer than 2^32 roles per block");
                    let role = *roles.entry(member.role).or_insert(next);
                    found.members.push(Found {
                        kind,
                        member: member.id,
                        set,
                        relation: relation.id(),
                        role,
                    });
                }
            }
        }
        let mut ordered: Vec<(&str, u32)> = roles.into_iter().collect();
        ordered.sort_unstable_by_key(|(_, position)| *position);
        found.roles.extend(ordered.into_iter().map(|(role, _)| role.to_owned()));
    }
}

impl SetFilter<'_> {
    /// Whether the set's `where` holds for `relation`, whose block gives the keys of the `where`'s tags.
    fn holds(&self, relation: &Relation<'_>, keys: &[Option<TagKey<'_>>], slots: &mut PropSlots) -> bool {
        let value = if self.columns.is_some() {
            let props: Vec<(KeyId, Prop<&str>)> = (0u32..)
                .zip(keys)
                .filter_map(|(pos, key)| {
                    let value = relation.tag((*key)?).filter(|v| !v.is_empty())?;
                    Some((KeyId::new(pos), Prop::Str(value)))
                })
                .collect();
            self.set.filter.eval(&slots.bind(&self.keys, &props))
        } else {
            let names: Vec<&str> = relation.tags().map(|(key, _)| key).collect();
            let keys = ExprKeys::columns(&names);
            let props: Vec<(KeyId, Prop<&str>)> = (0u32..)
                .zip(relation.tags())
                .filter(|(_, (_, value))| !value.is_empty())
                .map(|(pos, (_, value))| (KeyId::new(pos), Prop::Str(value)))
                .collect();
            self.set.filter.eval(&slots.bind(&keys, &props))
        };
        value == Ok(Value::Bool(true))
    }
}

/// What the relation pass found of the relation sets so far, block by block.
#[derive(Default)]
pub(super) struct FoundRelations {
    relations: Vec<SetRelation>,
    members: Vec<Found>,
    /// The roles of `members`, by the positions they give, repeated from block to block.
    roles: Vec<String>,
}

impl FoundRelations {
    /// Adds what another block found.
    pub(super) fn extend(&mut self, other: Self) {
        let offset = u32::try_from(self.roles.len()).expect("fewer than 2^32 roles");
        self.relations.extend(other.relations);
        self.members.extend(other.members.into_iter().map(|found| Found {
            role: found.role + offset,
            ..found
        }));
        self.roles.extend(other.roles);
    }
}

/// The relations of a table's relation sets, by set and id, and the nodes and ways that are their members.
#[derive(Default)]
pub(super) struct Members {
    /// The names of the sets, by position.
    sets: Vec<String>,
    relations: Vec<SetRelation>,
    /// The roles, sorted.
    roles: Vec<SourceStr>,
    /// The memberships of nodes, sorted.
    nodes: Vec<Entry>,
    /// The memberships of ways, sorted.
    ways: Vec<Entry>,
}

impl Members {
    /// The relations and members `found` holds, sorted: relations by set, then id, and each member's memberships
    /// by relation, then role, once each.
    pub(super) fn new(found: FoundRelations, sets: Vec<String>) -> Self {
        let FoundRelations {
            mut relations,
            members,
            roles: found_roles,
        } = found;
        relations.sort_unstable_by_key(|relation| (relation.set, relation.id));
        relations.dedup_by_key(|relation| (relation.set, relation.id));
        let mut roles: Vec<&str> = found_roles.iter().map(String::as_str).collect();
        roles.sort_unstable();
        roles.dedup();
        let role_position: Vec<u32> = found_roles
            .iter()
            .map(|role| {
                let position = roles.binary_search(&role.as_str()).expect("every role is listed");
                u32::try_from(position).expect("fewer than 2^32 roles")
            })
            .collect();
        let (mut nodes, mut ways) = (Vec::new(), Vec::new());
        for found in members {
            let position = relations
                .binary_search_by_key(&(found.set, found.relation), |relation| (relation.set, relation.id))
                .expect("every member's relation is listed");
            let entry = Entry {
                member: found.member,
                relation: u32::try_from(position).expect("fewer than 2^32 relations"),
                role: role_position[found.role as usize],
            };
            match found.kind {
                MemberKind::Node => nodes.push(entry),
                MemberKind::Way => ways.push(entry),
            }
        }
        for entries in [&mut nodes, &mut ways] {
            entries.sort_unstable();
            entries.dedup();
            entries.shrink_to_fit();
        }
        Self {
            sets,
            relations,
            roles: roles.into_iter().map(SourceStr::from).collect(),
            nodes,
            ways,
        }
    }

    /// How many relations the sets hold.
    pub(super) fn relations(&self) -> usize {
        self.relations.len()
    }

    /// How many memberships of nodes and ways the sets' relations have.
    pub(super) fn memberships(&self) -> usize {
        self.nodes.len() + self.ways.len()
    }

    /// The memberships of the member of type `kind` with OSM id `id`.
    fn entries(&self, kind: MemberKind, id: i64) -> &[Entry] {
        let entries = match kind {
            MemberKind::Node => &self.nodes,
            MemberKind::Way => &self.ways,
        };
        let start = entries.partition_point(|entry| entry.member < id);
        let end = start + entries[start..].partition_point(|entry| entry.member == id);
        &entries[start..end]
    }

    /// Whether the object of type `kind` with OSM id `id` is a member of a relation of the set named `set`.
    pub(super) fn is_member(&self, kind: MemberKind, id: i64, set: &str) -> bool {
        let Some(set) = self.sets.iter().position(|name| name == set) else {
            return false;
        };
        self.entries(kind, id)
            .iter()
            .any(|entry| usize::from(self.relations[entry.relation as usize].set) == set)
    }

    /// Whether the object of type `kind` with OSM id `id` is a member of a relation of some set.
    pub(super) fn has(&self, kind: MemberKind, id: i64) -> bool {
        !self.entries(kind, id).is_empty()
    }

    /// The relations the object of type `kind` with OSM id `id` is a member of, with their text copied by `copy`.
    pub(super) fn of(&self, kind: MemberKind, id: i64, copy: &mut impl FnMut(&str) -> SourceStr) -> Vec<Membership> {
        self.entries(kind, id)
            .iter()
            .map(|entry| {
                let relation = &self.relations[entry.relation as usize];
                Membership {
                    set: relation.set,
                    id: relation.id,
                    role: self.roles[entry.role as usize].clone(),
                    tags: relation.tags.iter().map(|tag| tag.as_deref().map(&mut *copy)).collect(),
                }
            })
            .collect()
    }
}
