//! Ordered parallel map: the caller produces batches in tile order, worker threads encode them in any
//! order, and one writer thread consumes the results in production order.

use std::thread::{self, Scope, ScopedJoinHandle};

use hotpath::wrap::flume::Sender as ProfiledSender;

use crate::{TileGenError, TileGenResult};

type Pending<O> = flume::Receiver<TileGenResult<O>>;
type Work<I, O> = (I, flume::Sender<TileGenResult<O>>);

pub(crate) fn spawn_named<'scope, T: Send + 'scope>(
    scope: &'scope Scope<'scope, '_>,
    role: &str,
    index: usize,
    task: impl FnOnce() -> T + Send + 'scope,
) -> ScopedJoinHandle<'scope, T> {
    thread::Builder::new()
        .name(format!("{role}-{index}"))
        .spawn_scoped(scope, task)
        .expect("spawning a pipeline thread")
}

/// Hands batches to the encoders, blocking while `window` batches are already in flight.
pub struct BatchSender<I, O> {
    work: ProfiledSender<Work<I, O>>,
    order: ProfiledSender<Pending<O>>,
}

impl<I, O> BatchSender<I, O> {
    /// Fails only when the writer stopped early; its own error is what [`run`] returns then.
    pub fn send(&self, batch: I) -> TileGenResult<()> {
        let (result_tx, result_rx) = flume::bounded(1);
        hotpath::measure_block!(
            { "submit_order_send" },
            self.order
                .send(result_rx)
                .map_err(|_closed| TileGenError::WriterStopped)
        )?;
        self.work
            .send((batch, result_tx))
            .map_err(|_closed| TileGenError::WriterStopped)
    }

    fn fail(&self, err: TileGenError) {
        let (result_tx, result_rx) = flume::bounded(1);
        let _ = result_tx.send(Err(err));
        let _ = self.order.send(result_rx);
    }
}

/// Runs `produce` on the calling thread, `encode` on `threads` workers (each with its own state from
/// `init`, returned at the end, e.g. for statistics) and `write` on its own thread.
/// At most `window` batches wait to be written besides the one being written, which bounds memory even
/// when one batch is slow. A producer or encoder error reaches the writer in order as an `Err` item, so a
/// sink never finishes a partial output; the first root-cause error is returned.
pub fn run<I: Send + 'static, O: Send + 'static, S: Send, W: Send>(
    threads: usize,
    window: usize,
    produce: impl FnOnce(&BatchSender<I, O>) -> TileGenResult<()>,
    init: impl Fn() -> S + Sync,
    encode: impl Fn(&mut S, I) -> TileGenResult<O> + Sync,
    write: impl FnOnce(&mut dyn Iterator<Item = TileGenResult<O>>) -> TileGenResult<W> + Send,
) -> TileGenResult<(W, Vec<S>)> {
    let (work_tx, work_rx) =
        hotpath::channel!(flume::unbounded::<Work<I, O>>(), label = { "encode_work" });
    let (order_tx, order_rx) = hotpath::channel!(
        flume::bounded::<Pending<O>>(window),
        label = { "encode_order" }
    );
    thread::scope(|scope| {
        let encoders: Vec<_> = (0..threads.max(1))
            .map(|i| {
                let (work_rx, init, encode) = (work_rx.clone(), &init, &encode);
                spawn_named(scope, "encoder", i, move || {
                    let mut state = init();
                    for (batch, result_tx) in work_rx {
                        let _ = result_tx.send(encode(&mut state, batch));
                    }
                    state
                })
            })
            .collect();
        drop(work_rx);
        let writer = spawn_named(scope, "writer", 0, move || {
            write(&mut order_rx.into_iter().map_while(|pending| {
                hotpath::measure_block!({ "writer_wait_result" }, pending.recv().ok())
            }))
        });

        let sender = BatchSender {
            work: work_tx,
            order: order_tx,
        };
        if let Err(err) = produce(&sender)
            && !matches!(err, TileGenError::WriterStopped)
        {
            sender.fail(err);
        }
        drop(sender);
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    fn produce_n(n: usize) -> impl FnOnce(&BatchSender<usize, usize>) -> TileGenResult<()> {
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
        assert_eq!(counts.len(), 4);
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
        let produce = |submit: &BatchSender<usize, usize>| {
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
        let sent = AtomicUsize::new(0);
        let produce = |submit: &BatchSender<usize, usize>| {
            (0..1_000_000).try_for_each(|i| {
                sent.fetch_add(1, Ordering::Relaxed);
                submit.send(i)
            })
        };

        let err = run(
            2,
            2,
            produce,
            || (),
            |(), i| Ok(i),
            |results| {
                results.take(3).try_for_each(|r| r.map(drop))?;
                Err::<(), _>(TileGenError::InvalidTileId(1))
            },
        )
        .unwrap_err();

        assert!(matches!(err, TileGenError::InvalidTileId(1)));
        assert!(sent.into_inner() < 1_000_000);
    }
}
