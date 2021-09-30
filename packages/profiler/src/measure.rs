use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{self, Duration};

use crate::code_blocks::{BlockId, BlockStore};
use crate::utils::InsertPush as _;

use itertools::Itertools;
use wasmer::WasmerEnv;

#[derive(Default, Debug, Clone, WasmerEnv)]
pub struct Measurements {
    measurements: Vec<Measurement>,
}

impl Measurements {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns an execution number used to identify this measurement. The Wasm code
    /// will later supply this same identifier via `take_measurement`.
    pub fn start_measurement(&mut self) -> u32 {
        self.measurements.push(Measurement::new());
        self.measurements.len() as u32 - 1
    }

    // TODO: Error handling? This will be called from Wasm code probably.
    pub fn take_measurement(&mut self, execution: u32, block_id: impl Into<BlockId>) {
        self.measurements[execution as usize].take(block_id);
    }

    pub fn compile_results(&mut self) -> Results {
        Results {
            data: self
                .measurements
                .drain(..)
                .filter_map(|ms| match ms {
                    Measurement::Started(_) => {
                        eprintln!("warning: a measurement was started, but not finalized");
                        None
                    }
                    Measurement::Taken(block_id, duration) => Some((block_id, duration)),
                })
                .into_group_map(),
        }
    }

    pub fn clear(&mut self) {
        self.measurements = Vec::new();
    }
}

#[derive(Debug, Clone)]
enum Measurement {
    Started(time::Instant),
    Taken(BlockId, time::Duration),
}

impl Measurement {
    pub fn new() -> Self {
        Self::Started(time::Instant::now())
    }

    pub fn take(&mut self, block_id: impl Into<BlockId>) {
        match self {
            Measurement::Started(start) => *self = Self::Taken(block_id.into(), start.elapsed()),
            Measurement::Taken(_, _) => {
                panic!("attempt to take a measurement that was already taken")
            }
        }
    }
}

impl WasmerEnv for Measurement {}

#[derive(Clone, Debug)]
pub struct Results {
    data: HashMap<BlockId, Vec<time::Duration>>,
}

impl Results {
    pub fn compile_csv(&self, block_store: Arc<Mutex<BlockStore>>, sink: impl std::io::Write) {
        let block_store = block_store.lock().unwrap();
        let mut wtr = csv::WriterBuilder::new()
            .terminator(csv::Terminator::CRLF)
            .flexible(true)
            .from_writer(sink);

        // Header row
        wtr.write_record(["block", "executions", "avg in ns", "min in ns", "max in ns"])
            .unwrap();

        for (block_id, timings) in &self.data {
            let avg = timings.iter().sum::<Duration>().as_nanos() / timings.len() as u128;
            let min = timings.iter().min().unwrap().as_nanos();
            let max = timings.iter().max().unwrap().as_nanos();
            let executions = timings.len();

            let block = format!("{:?}", block_store.get_block(*block_id).unwrap());
            wtr.write_record([
                block,
                executions.to_string(),
                avg.to_string(),
                min.to_string(),
                max.to_string(),
            ])
            .unwrap();

            // wtr.write_record(timings.iter().map(|d| d.as_nanos().to_string()))
            //     .unwrap();
        }

        wtr.flush().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_measurements_of_different_blocks() {
        // TODO: This is probably very confusing. What's a good way to refactor?

        let mut measure = Measurements::new();

        let ms0 = measure.start_measurement();
        let ms1 = measure.start_measurement();
        std::thread::sleep(time::Duration::from_millis(100));
        let ms2 = measure.start_measurement();
        let _ms3 = measure.start_measurement();

        measure.take_measurement(ms0, 0);
        measure.take_measurement(ms1, 1);
        measure.take_measurement(ms2, 0);

        assert_eq!(measure.measurements.len(), 4);

        let results = measure.compile_results();

        assert!(results.data[&BlockId(0)][0] > time::Duration::from_millis(100));
    }
}
