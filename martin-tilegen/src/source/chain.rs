//! Several sources read as one: their tables and partitions numbered one source after another.

use std::fmt;

use crate::plan::{Plan, PlanError, TableDef};
use crate::props::KeyInterner;
use crate::source::{FeatureBatch, FeatureReader, FeatureSource, SourceError, SourceResult};

/// Sources read as one, under one plan of all their tables. The tables and the partitions of each source come after
/// those of the sources before it, so a source's layers and its features follow those of the sources before it.
pub struct SourceChain {
    plan: Plan,
    parts: Vec<Part>,
    partitions: u32,
}

/// One source of a chain, and where its tables and partitions start in the chain's numbering.
struct Part {
    source: Box<dyn FeatureSource + Send>,
    /// The plan's number of the source's first table.
    first_table: usize,
    /// How many tables the source reads.
    tables: usize,
    /// The chain's number of the source's first partition.
    first_partition: u32,
}

impl SourceChain {
    /// Chains `sources` in the order given, each with the tables it was made to read, in its own order.
    ///
    /// # Errors
    /// If the tables of all sources together are not a valid plan, e.g. when two have a layer of the same name.
    pub fn new(sources: Vec<(Box<dyn FeatureSource + Send>, Vec<TableDef>)>) -> Result<Self, PlanError> {
        let mut defs = Vec::new();
        let mut parts = Vec::with_capacity(sources.len());
        let mut partitions = 0u32;
        for (source, tables) in sources {
            parts.push(Part {
                first_table: defs.len(),
                tables: tables.len(),
                first_partition: partitions,
                source,
            });
            partitions = partitions
                .checked_add(parts.last().map_or(0, |part| part.source.partitions()))
                .expect("fewer than 2^32 partitions");
            defs.extend(tables);
        }
        Ok(Self {
            plan: Plan::new(defs)?,
            parts,
            partitions,
        })
    }

    /// The plan of every source's tables, which the chain's batches are numbered for.
    #[must_use]
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// The position of the part that reads `partition`.
    fn part_of(&self, partition: u32) -> SourceResult<usize> {
        if partition >= self.partitions {
            return Err(SourceError::UnknownPartition(partition));
        }
        Ok(self.parts.partition_point(|part| part.first_partition <= partition) - 1)
    }
}

impl Part {
    /// Reads the chain's `partition` through `reader`, or the source itself without one, renumbering its batches.
    fn read(
        &self,
        reader: Option<&mut (dyn FeatureReader + '_)>,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        let keys = keys
            .get(self.first_table..self.first_table + self.tables)
            .expect("the keys are those of the chain's plan");
        let mut renumbered = |mut batch: FeatureBatch| {
            batch.table = u16::try_from(self.first_table + usize::from(batch.table))
                .expect("the plan has fewer than 2^16 tables");
            batch.partition = partition;
            emit(batch)
        };
        let own = partition - self.first_partition;
        match reader {
            Some(reader) => reader.read(own, keys, &mut renumbered),
            None => self.source.read(own, keys, &mut renumbered),
        }
    }
}

impl FeatureSource for SourceChain {
    fn partitions(&self) -> u32 {
        self.partitions
    }

    /// The sum of every source's estimate, if each has one.
    fn estimated_features(&self) -> Option<u64> {
        self.parts.iter().map(|part| part.source.estimated_features()).sum()
    }

    fn read(
        &self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        self.parts[self.part_of(partition)?].read(None, partition, keys, emit)
    }

    /// As many readers as the source with partitions that opens the most, each holding one reader of every such
    /// source; a reader that a source opened none for reads that source's partitions through [`FeatureSource::read`].
    fn open_readers(&self, count: usize) -> SourceResult<Vec<Box<dyn FeatureReader + '_>>> {
        let mut opened = Vec::with_capacity(self.parts.len());
        for part in &self.parts {
            let readers = if part.source.partitions() == 0 {
                Vec::new()
            } else {
                part.source.open_readers(count)?
            };
            opened.push(readers);
        }
        let most = opened.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let mut opened: Vec<_> = opened.into_iter().map(Vec::into_iter).collect();
        Ok((0..most)
            .map(|_| {
                let readers = opened.iter_mut().map(Iterator::next).collect();
                Box::new(ChainReader { chain: self, readers }) as Box<dyn FeatureReader + '_>
            })
            .collect())
    }

    /// Finishes every source, in order, failing on the first that fails.
    fn finish(&self) -> SourceResult<()> {
        self.parts.iter().try_for_each(|part| part.source.finish())
    }
}

/// One reader thread of a chain: a reader of each source, where the source gave one.
struct ChainReader<'a> {
    chain: &'a SourceChain,
    readers: Vec<Option<Box<dyn FeatureReader + 'a>>>,
}

impl FeatureReader for ChainReader<'_> {
    fn read(
        &mut self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        let index = self.chain.part_of(partition)?;
        let reader = self.readers[index].as_deref_mut();
        self.chain.parts[index].read(reader, partition, keys, emit)
    }
}

impl fmt::Debug for SourceChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceChain")
            .field("sources", &self.parts.len())
            .field("partitions", &self.partitions)
            .finish_non_exhaustive()
    }
}
