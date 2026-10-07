#![allow(clippy::unwrap_used)]
use std::sync::{Arc, Mutex};

use martin_tilegen::{
    DedupHint, EncodedTile, OrderedWriter, TileGenError, TileGenResult, TileOrder, TileSink,
};
use tilejson::TileJSON;

const TILE_COUNT: u64 = 1365;

#[derive(Default)]
struct Recorded {
    tiles: Vec<(u64, DedupHint)>,
    finished_with: Option<TileJSON>,
}

struct RecordingSink {
    order: TileOrder,
    fail_on_batch: Option<usize>,
    batches_seen: usize,
    recorded: Arc<Mutex<Recorded>>,
}

impl RecordingSink {
    fn new(order: TileOrder) -> (Self, Arc<Mutex<Recorded>>) {
        let recorded = Arc::default();
        let sink = Self {
            order,
            fail_on_batch: None,
            batches_seen: 0,
            recorded: Arc::clone(&recorded),
        };
        (sink, recorded)
    }
}

impl TileSink for RecordingSink {
    fn tile_order(&self) -> TileOrder {
        self.order
    }

    fn write(&mut self, batch: &[EncodedTile]) -> TileGenResult<()> {
        if self.fail_on_batch == Some(self.batches_seen) {
            return Err(TileGenError::InvalidSortConfig("sink refused the batch"));
        }
        self.batches_seen += 1;
        let mut recorded = self.recorded.lock().unwrap();
        for tile in batch {
            let id = self.order.tile_id(tile.coord)?;
            recorded.tiles.push((id, tile.hint));
        }
        Ok(())
    }

    fn finish(self, meta: &TileJSON) -> TileGenResult<()> {
        self.recorded.lock().unwrap().finished_with = Some(meta.clone());
        Ok(())
    }
}

fn hint_for(id: u64) -> DedupHint {
    if id.is_multiple_of(3) {
        DedupHint::LikelyDuplicate
    } else {
        DedupHint::Unique
    }
}

fn hilbert_batches(batch_len: usize) -> Vec<Vec<EncodedTile>> {
    let tiles: Vec<_> = (0..TILE_COUNT)
        .map(|id| EncodedTile {
            coord: TileOrder::Hilbert.tile_coord(id).unwrap(),
            data: id.to_le_bytes().to_vec(),
            hint: hint_for(id),
        })
        .collect();
    tiles.chunks(batch_len).map(<[_]>::to_vec).collect()
}

fn meta() -> TileJSON {
    tilejson::tilejson! { tiles: vec![] }
}

#[test]
fn reversed_batches_reach_the_sink_in_hilbert_order() {
    let (sink, recorded) = RecordingSink::new(TileOrder::Hilbert);
    let writer = OrderedWriter::spawn_sink(2, sink, meta()).unwrap();
    for (seq, batch) in hilbert_batches(37).into_iter().enumerate().rev() {
        writer.submit(seq as u64, batch).unwrap();
    }
    writer.finish().unwrap();

    let recorded = recorded.lock().unwrap();
    let ids: Vec<_> = recorded.tiles.iter().map(|&(id, _)| id).collect();
    assert_eq!(ids, (0..TILE_COUNT).collect::<Vec<_>>());
    assert!(recorded.finished_with.is_some());
}

#[test]
fn parallel_producers_keep_hilbert_order() {
    let (sink, recorded) = RecordingSink::new(TileOrder::Hilbert);
    let writer = OrderedWriter::spawn_sink(3, sink, meta()).unwrap();
    let batches = hilbert_batches(11);
    let threads = 4;
    std::thread::scope(|scope| {
        for t in 0..threads {
            let (writer, batches) = (&writer, &batches);
            scope.spawn(move || {
                for seq in (t..batches.len()).step_by(threads).rev() {
                    writer.submit(seq as u64, batches[seq].clone()).unwrap();
                }
            });
        }
    });
    writer.finish().unwrap();

    let recorded = recorded.lock().unwrap();
    let ids: Vec<_> = recorded.tiles.iter().map(|&(id, _)| id).collect();
    assert_eq!(ids, (0..TILE_COUNT).collect::<Vec<_>>());
}

#[test]
fn dedup_hints_pass_through() {
    let (sink, recorded) = RecordingSink::new(TileOrder::Hilbert);
    let writer = OrderedWriter::spawn_sink(4, sink, meta()).unwrap();
    for (seq, batch) in hilbert_batches(100).into_iter().enumerate() {
        writer.submit(seq as u64, batch).unwrap();
    }
    writer.finish().unwrap();

    let expected: Vec<_> = (0..TILE_COUNT).map(|id| (id, hint_for(id))).collect();
    assert_eq!(recorded.lock().unwrap().tiles, expected);
}

#[test]
fn a_missing_batch_fails_the_write() {
    let (sink, _) = RecordingSink::new(TileOrder::Hilbert);
    let writer = OrderedWriter::spawn_sink(4, sink, meta()).unwrap();
    let mut batches = hilbert_batches(100);
    writer.submit(2, batches.remove(2)).unwrap();
    writer.submit(0, batches.remove(0)).unwrap();
    assert!(matches!(
        writer.finish(),
        Err(TileGenError::MissingBatch(1))
    ));
}

#[test]
fn a_repeated_batch_number_fails_the_write() {
    let (sink, _) = RecordingSink::new(TileOrder::Hilbert);
    let writer = OrderedWriter::spawn_sink(4, sink, meta()).unwrap();
    let batches = hilbert_batches(100);
    writer.submit(1, batches[1].clone()).unwrap();
    writer.submit(1, batches[1].clone()).unwrap();
    assert!(matches!(
        writer.finish(),
        Err(TileGenError::DuplicateBatch(1))
    ));
}

#[test]
fn tiles_outside_the_sink_order_fail_the_write() {
    let (sink, _) = RecordingSink::new(TileOrder::Tms);
    let writer = OrderedWriter::spawn_sink(4, sink, meta()).unwrap();
    for (seq, batch) in hilbert_batches(100).into_iter().enumerate() {
        if writer.submit(seq as u64, batch).is_err() {
            break;
        }
    }
    assert!(matches!(
        writer.finish(),
        Err(TileGenError::NotAscending(_))
    ));
}

#[test]
fn a_sink_error_is_returned_by_finish() {
    let (mut sink, _) = RecordingSink::new(TileOrder::Hilbert);
    sink.fail_on_batch = Some(1);
    let writer = OrderedWriter::spawn_sink(1, sink, meta()).unwrap();
    for (seq, batch) in hilbert_batches(10).into_iter().enumerate() {
        if writer.submit(seq as u64, batch).is_err() {
            break;
        }
    }
    assert!(matches!(
        writer.finish(),
        Err(TileGenError::InvalidSortConfig("sink refused the batch"))
    ));
}
