//! Ordered parallel map: the caller produces batches in tile order, worker threads encode them in any
//! order, and one writer thread consumes the results in production order.

use std::collections::BTreeMap;
use std::thread;

use crate::{TileGenError, TileGenResult};

type Indexed<T> = (usize, TileGenResult<T>);

/// Hands batches to the encoders, waiting while `window` batches are already in flight.
pub struct Submit<I> {
    work: flume::Sender<(usize, I)>,
    credits: flume::Receiver<()>,
    next: usize,
}

impl<I> Submit<I> {
    /// Fails only when the writer stopped early; its own error is what [`run`] returns then.
    pub fn send(&mut self, batch: I) -> TileGenResult<()> {
        self.credits
            .recv()
            .map_err(|_closed| TileGenError::WriterStopped)?;
        self.work
            .send((self.next, batch))
            .map_err(|_closed| TileGenError::WriterStopped)?;
        self.next += 1;
        Ok(())
    }
}

/// Runs `produce` on the calling thread, `encode` on `threads` workers (each with its own state from
/// `init`, returned at the end, e.g. for statistics) and `write` on its own thread.
/// At most `window` batches are produced but not yet written, which bounds memory even when one batch
/// is slow. A producer or encoder error reaches the writer in order as an `Err` item, so a sink never
/// finishes a partial output; the first root-cause error is returned.
pub fn run<I: Send, O: Send, S: Send, W: Send>(
    threads: usize,
    window: usize,
    produce: impl FnOnce(&mut Submit<I>) -> TileGenResult<()>,
    init: impl Fn() -> S + Sync,
    encode: impl Fn(&mut S, I) -> TileGenResult<O> + Sync,
    write: impl FnOnce(&mut dyn Iterator<Item = TileGenResult<O>>) -> TileGenResult<W> + Send,
) -> TileGenResult<(W, Vec<S>)> {
    let (work_tx, work_rx) = flume::bounded::<(usize, I)>(window);
    let (result_tx, result_rx) = flume::unbounded::<Indexed<O>>();
    let (credit_tx, credit_rx) = flume::bounded(window);
    for _ in 0..window {
        credit_tx
            .send(())
            .map_err(|_closed| TileGenError::WriterStopped)?;
    }
    thread::scope(|scope| {
        let encoders: Vec<_> = (0..threads.max(1))
            .map(|_| {
                let (work_rx, result_tx, init, encode) =
                    (work_rx.clone(), result_tx.clone(), &init, &encode);
                scope.spawn(move || {
                    let mut state = init();
                    for (index, batch) in work_rx {
                        if result_tx.send((index, encode(&mut state, batch))).is_err() {
                            break;
                        }
                    }
                    state
                })
            })
            .collect();
        drop(work_rx);
        let writer = scope.spawn(move || {
            write(&mut InOrder {
                results: result_rx,
                credits: credit_tx,
                pending: BTreeMap::new(),
                next: 0,
            })
        });

        let mut submit = Submit {
            work: work_tx,
            credits: credit_rx,
            next: 0,
        };
        if let Err(err) = produce(&mut submit)
            && !matches!(err, TileGenError::WriterStopped)
        {
            let _ = result_tx.send((submit.next, Err(err)));
        }
        drop((submit, result_tx));
        let written = writer
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))?;
        let states = encoders
            .into_iter()
            .map(|e| {
                e.join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect();
        Ok((written, states))
    })
}

struct InOrder<O> {
    results: flume::Receiver<Indexed<O>>,
    credits: flume::Sender<()>,
    pending: BTreeMap<usize, TileGenResult<O>>,
    next: usize,
}

impl<O> Iterator for InOrder<O> {
    type Item = TileGenResult<O>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(result) = self.pending.remove(&self.next) {
                self.next += 1;
                let _ = self.credits.send(());
                return Some(result);
            }
            let (index, result) = self.results.recv().ok()?;
            self.pending.insert(index, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn produce_n(n: usize) -> impl FnOnce(&mut Submit<usize>) -> TileGenResult<()> {
        move |submit| (0..n).try_for_each(|i| submit.send(i))
    }

    /// Later batches finish first, so only the reordering makes the output sorted.
    fn slow_early(i: usize) -> usize {
        thread::sleep(Duration::from_micros(((50 - i % 50) * 20) as u64));
        i * 10
    }

    #[test]
    fn writes_in_production_order() {
        let (out, counts) = run(
            4,
            8,
            produce_n(500),
            || 0,
            |count: &mut usize, i| {
                *count += 1;
                Ok(slow_early(i))
            },
            |results| results.collect::<TileGenResult<Vec<_>>>(),
        )
        .unwrap();
        assert_eq!(out, (0..500).map(|i| i * 10).collect::<Vec<_>>());
        assert_eq!(counts.iter().sum::<usize>(), 500);
    }

    #[test]
    fn encoder_error_stops_the_writer_in_order() {
        let encode = |(): &mut (), i| {
            if i == 7 {
                Err(TileGenError::RecordTooLarge(i))
            } else {
                Ok(slow_early(i))
            }
        };
        let mut seen = 0;
        let err = run(
            4,
            8,
            produce_n(100),
            || (),
            encode,
            |results| {
                for result in results {
                    result?;
                    seen += 1;
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(matches!(err, TileGenError::RecordTooLarge(7)));
        assert_eq!(seen, 7);
    }

    #[test]
    fn producer_error_reaches_the_writer_after_submitted_batches() {
        let produce = |submit: &mut Submit<usize>| {
            (0..5).try_for_each(|i| submit.send(i))?;
            Err(TileGenError::InvalidTileId(99))
        };
        let mut seen = Vec::new();
        let err = run(
            2,
            3,
            produce,
            || (),
            |(), i| Ok(i),
            |results| {
                for result in results {
                    seen.push(result?);
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(matches!(err, TileGenError::InvalidTileId(99)));
        assert_eq!(seen, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn writer_error_stops_the_producer() {
        let err = run(
            2,
            2,
            produce_n(1_000_000),
            || (),
            |(), i| Ok(i),
            |results| {
                results.take(3).try_for_each(|r| r.map(drop))?;
                Err::<(), _>(TileGenError::InvalidTileId(1))
            },
        )
        .unwrap_err();
        assert!(matches!(err, TileGenError::InvalidTileId(1)));
    }
}
