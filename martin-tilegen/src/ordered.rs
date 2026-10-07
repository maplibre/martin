use std::collections::BTreeMap;
use std::thread::JoinHandle;

use tilejson::TileJSON;

use crate::{EncodedTile, TileGenError, TileGenResult, TileOrder, TileSink};

type Numbered = (u64, Vec<EncodedTile>);

/// Runs a writer on one dedicated thread, fed batches by any number of producers in any order.
///
/// Batch numbers start at 0 and have no gaps. The writer sees the batches in number order, and
/// checks that tile ids ascend across them.
pub struct OrderedWriter {
    sender: flume::Sender<Numbered>,
    thread: JoinHandle<TileGenResult<()>>,
}

impl OrderedWriter {
    /// Starts a thread that hands `body` the batches in order. `capacity` bounds the batches queued
    /// for the thread, which blocks [`Self::submit`] while the thread is behind.
    pub fn spawn<F>(order: TileOrder, capacity: usize, body: F) -> TileGenResult<Self>
    where
        F: FnOnce(OrderedBatches) -> TileGenResult<()> + Send + 'static,
    {
        let (sender, receiver) = flume::bounded(capacity);
        let batches = OrderedBatches {
            receiver,
            pending: BTreeMap::new(),
            next: 0,
            order,
            last_id: None,
        };
        let thread = std::thread::Builder::new()
            .name("tile-writer".to_owned())
            .spawn(move || body(batches))?;
        Ok(Self { sender, thread })
    }

    /// Starts a thread that writes the batches to `sink` and finishes it with `meta`.
    pub fn spawn_sink<S>(capacity: usize, mut sink: S, meta: TileJSON) -> TileGenResult<Self>
    where
        S: TileSink + 'static,
    {
        Self::spawn(sink.tile_order(), capacity, move |mut batches| {
            while let Some(batch) = batches.next_batch()? {
                sink.write(&batch)?;
            }
            sink.finish(&meta)
        })
    }

    /// Queues a batch. An error means the writer stopped, and [`Self::finish`] returns why.
    pub fn submit(&self, seq: u64, batch: Vec<EncodedTile>) -> TileGenResult<()> {
        match self.sender.send((seq, batch)) {
            Ok(()) => Ok(()),
            Err(flume::SendError(_)) => Err(TileGenError::WriterStopped),
        }
    }

    /// Waits for the writer to drain every queued batch and returns its result.
    pub fn finish(self) -> TileGenResult<()> {
        drop(self.sender);
        self.thread
            .join()
            .unwrap_or(Err(TileGenError::WriterPanicked))
    }
}

/// The batches given to an [`OrderedWriter`], in batch number order.
pub struct OrderedBatches {
    receiver: flume::Receiver<Numbered>,
    pending: BTreeMap<u64, Vec<EncodedTile>>,
    next: u64,
    order: TileOrder,
    last_id: Option<u64>,
}

impl OrderedBatches {
    /// Blocks until the next batch in order arrives; `None` once every producer is done.
    pub fn next_batch(&mut self) -> TileGenResult<Option<Vec<EncodedTile>>> {
        loop {
            if let Some(batch) = self.pending.remove(&self.next) {
                self.check_ascending(&batch)?;
                self.next += 1;
                return Ok(Some(batch));
            }
            match self.receiver.recv() {
                Ok((seq, batch)) => {
                    if seq < self.next || self.pending.insert(seq, batch).is_some() {
                        return Err(TileGenError::DuplicateBatch(seq));
                    }
                }
                Err(_) if self.pending.is_empty() => return Ok(None),
                Err(_) => return Err(TileGenError::MissingBatch(self.next)),
            }
        }
    }

    fn check_ascending(&mut self, batch: &[EncodedTile]) -> TileGenResult<()> {
        for tile in batch {
            let id = self.order.tile_id(tile.coord)?;
            if self.last_id.is_some_and(|last| id <= last) {
                return Err(TileGenError::NotAscending(tile.coord));
            }
            self.last_id = Some(id);
        }
        Ok(())
    }
}
