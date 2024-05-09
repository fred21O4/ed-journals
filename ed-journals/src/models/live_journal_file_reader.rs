use std::collections::VecDeque;
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::Thread;
use std::{io, thread};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

use crate::models::journal_file_reader::JournalReaderError;
use crate::{JournalEvent, JournalFileReader};

/// Allows you to iterate over a journal log file and blocks when there are no entries to read, then
/// when the file changes it will unblock and return the new line(s).
///
/// ```no_run
/// use std::path::PathBuf;
/// use ed_journals::LiveJournalFileReader;
///
/// let path = PathBuf::from("somePath");
///
/// let live_reader = LiveJournalFileReader::new(path)
///     .unwrap();
///
/// // This will block the current thread until there are new entries.
/// for entry in live_reader {
///     // Do something with the entry
/// }
/// ```
#[derive(Debug)]
pub struct LiveJournalFileReader {
    waiting_thread: Arc<Mutex<(Option<Thread>,)>>,
    journal_file_reader: JournalFileReader<File>,
    watcher: RecommendedWatcher,
    active: Arc<AtomicBool>,
}

#[derive(Debug, Error)]
pub enum LiveJournalFileReaderError {
    #[error(transparent)]
    IO(#[from] io::Error),

    #[error(transparent)]
    NotifyError(#[from] notify::Error),
}

impl LiveJournalFileReader {
    pub fn new(path: PathBuf) -> Result<Self, LiveJournalFileReaderError> {
        let file = File::open(&path)?;
        let journal_file_reader = JournalFileReader::from(file);

        let waiting_thread = Arc::new(Mutex::new((None::<Thread>,)));
        let waiting_thread_local = waiting_thread.clone();

        let mut watcher = notify::recommended_watcher(move |_| {
            let guard = waiting_thread_local
                .lock()
                .expect("Should have been locked");

            if let Some(thread) = guard.0.as_ref() {
                thread.unpark();
            };
        })?;

        watcher.watch(&path, RecursiveMode::NonRecursive)?;

        Ok(LiveJournalFileReader {
            waiting_thread,
            journal_file_reader,
            watcher,
            active: Arc::new(AtomicBool::new(true)),
        })
    }

    pub fn handle(&self) -> LiveJournalFileHandle {
        LiveJournalFileHandle {
            active: self.active.clone(),
            waiting_thread: self.waiting_thread.clone(),
        }
    }
}

pub struct LiveJournalFileHandle {
    active: Arc<AtomicBool>,
    waiting_thread: Arc<Mutex<(Option<Thread>,)>>,
}

impl LiveJournalFileHandle {
    pub fn close(&self) {
        self.active.swap(false, Ordering::Relaxed);
        let guard = self.waiting_thread.lock().expect("to have gotten a lock");

        if let Some(a) = guard.0.as_ref() {
            a.unpark();
        };
    }
}

impl Iterator for LiveJournalFileReader {
    type Item = Result<JournalEvent, JournalReaderError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if !self.active.load(Ordering::Relaxed) {
                return None;
            }

            match self.journal_file_reader.next() {
                Some(value) => return Some(value),
                None => {
                    {
                        let mut guard = self.waiting_thread.lock().expect("to have gotten a lock");

                        guard.0 = Some(thread::current());
                    }

                    thread::park();
                }
            }
        }
    }
}
