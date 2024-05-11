use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use notify::event::{DataChange, ModifyKind};
use thiserror::Error;
use crate::journal::journal_event::JournalEvent;
use crate::logs::asynchronous::{LogDirReader, LogDirReaderError};
use crate::modules::blockers::async_blocker::AsyncBlocker;
use crate::status::blocking::{read_status_file, ReadStatusFileError};
use crate::status::Status;

#[derive(Debug)]
pub struct LiveJournalDirReader {
    blocker: AsyncBlocker,
    watcher: RecommendedWatcher,
    log_dir_reader: LogDirReader,
    pending_events: Arc<Mutex<VecDeque<Result<JournalEvent, JournalDirWatcherError>>>>,
}

#[derive(Debug, Error)]
pub enum JournalDirWatcherError {
    #[error(transparent)]
    LogDirReaderError(#[from] LogDirReaderError),

    #[error(transparent)]
    ReadStatusFileError(#[from] ReadStatusFileError),

    #[error(transparent)]
    NotifyError(#[from] notify::Error),
}

impl LiveJournalDirReader {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, JournalDirWatcherError> {
        let blocker = AsyncBlocker::new();
        let local_blocker = blocker.clone();

        let dir_path = path.as_ref().to_path_buf();

        let pending_events = Arc::new(Mutex::new(VecDeque::new()));
        let local_pending_events = pending_events.clone();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res {
                let EventKind::Modify(ModifyKind::Data(DataChange::Content)) = event.kind else {
                    return;
                };

                for path in event.paths {
                    if path.ends_with("Status.json") {
                        local_pending_events.lock()
                            .expect("Failed to get lock")
                            .push_back(match read_status_file(dir_path.join("Status.json")) {
                                Ok(status) => Ok(JournalEvent::StatusEvent(status)),
                                Err(error) => Err(error.into()),
                            });
                    }
                }

                local_blocker.unblock_blocking();
            }
        })?;

        watcher.watch(path.as_ref(), RecursiveMode::NonRecursive)?;

        let log_dir_reader = LogDirReader::open(path);

        Ok(LiveJournalDirReader {
            blocker,
            watcher,
            log_dir_reader,
            pending_events,
        })
    }

    pub async fn next(&mut self) -> Option<Result<JournalEvent, JournalDirWatcherError>> {
        loop {
            if let Some(log_event) = self.log_dir_reader.next().await {
                return Some(match log_event {
                    Ok(event) => Ok(JournalEvent::LogEvent(event)),
                    Err(error) => Err(error.into()),
                })
            }

            let result = self.pending_events.lock()
                .expect("Failed to get lock")
                .pop_front();

            if result.is_none() {
                self.blocker.block().await;
                continue;
            }

            return result;
        }
    }
}

