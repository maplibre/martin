//! Criterion measurement in CPU cycles from `perf_event_open`; needs `kernel.perf_event_paranoid <= 2`.

use criterion::Throughput;
use criterion::measurement::{Measurement, ValueFormatter};
use perf_event::events::Hardware;
use perf_event::{Builder, Counter};

pub struct Cycles;

impl Measurement for Cycles {
    type Intermediate = Counter;
    type Value = u64;

    fn start(&self) -> Counter {
        let mut counter = Builder::new(Hardware::CPU_CYCLES)
            .inherit(true)
            .build()
            .expect("perf_event_open for CPU cycles (check kernel.perf_event_paranoid)");
        counter.enable().expect("enable the cycle counter");
        counter
    }

    fn end(&self, mut counter: Counter) -> u64 {
        counter.read().expect("read the cycle counter")
    }

    fn add(&self, a: &u64, b: &u64) -> u64 {
        a + b
    }

    fn zero(&self) -> u64 {
        0
    }

    #[expect(clippy::cast_precision_loss, reason = "statistics only")]
    fn to_f64(&self, value: &u64) -> f64 {
        *value as f64
    }

    fn formatter(&self) -> &dyn ValueFormatter {
        self
    }
}

impl ValueFormatter for Cycles {
    fn scale_values(&self, typical: f64, values: &mut [f64]) -> &'static str {
        let (factor, unit) = match typical {
            t if t >= 1e9 => (1e-9, "Gcycles"),
            t if t >= 1e6 => (1e-6, "Mcycles"),
            t if t >= 1e3 => (1e-3, "Kcycles"),
            _ => (1.0, "cycles"),
        };
        for v in values.iter_mut() {
            *v *= factor;
        }
        unit
    }

    #[expect(clippy::cast_precision_loss, reason = "statistics only")]
    fn scale_throughputs(
        &self,
        _typical: f64,
        throughput: &Throughput,
        values: &mut [f64],
    ) -> &'static str {
        let (count, unit) = match *throughput {
            Throughput::Elements(n) | Throughput::ElementsAndBytes { elements: n, .. } => {
                (n, "cycles/elem")
            }
            Throughput::Bytes(n) | Throughput::BytesDecimal(n) => (n, "cycles/byte"),
            Throughput::Bits(n) => (n, "cycles/bit"),
        };
        for v in values.iter_mut() {
            *v /= count as f64;
        }
        unit
    }

    fn scale_for_machines(&self, _values: &mut [f64]) -> &'static str {
        "cycles"
    }
}
