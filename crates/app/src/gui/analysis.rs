use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;

use core::PhotoAnalysis;
use vision::{analyze_folder, model_path as vision_model_path};

pub enum AnalysisEvent {
    ModelStatus(String),
    Progress { done: usize, total: usize },
    Done(Vec<PhotoAnalysis>),
    Failed(String),
}

pub struct AnalysisJob {
    #[allow(dead_code)]
    pub generation: u64,
    pub cancel: Arc<AtomicBool>,
    pub rx: Receiver<(u64, AnalysisEvent)>,
}

/// ONNX model path used for the pre-download status check.
pub fn model_path() -> PathBuf {
    vision_model_path()
}

/// Spawn a background thread. Caller owns `generation` (bump + cancel previous).
pub fn spawn_analyze(paths: Vec<PathBuf>, generation: u64) -> AnalysisJob {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let cancel_worker = Arc::clone(&cancel);
    thread::spawn(move || {
        let send = |event: AnalysisEvent| {
            let _ = tx.send((generation, event));
        };
        if !model_path().is_file() {
            send(AnalysisEvent::ModelStatus("Downloading model…".into()));
        }
        let result = analyze_folder(&paths, &cancel_worker, |done, total| {
            send(AnalysisEvent::Progress { done, total });
        });
        if cancel_worker.load(Ordering::Relaxed) {
            return;
        }
        match result {
            Ok(analyses) => send(AnalysisEvent::Done(analyses)),
            Err(e) => send(AnalysisEvent::Failed(e.to_string())),
        }
    });
    AnalysisJob {
        generation,
        cancel,
        rx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use vision::data_root;

    #[test]
    fn model_path_under_data_root() {
        assert_eq!(
            model_path(),
            data_root().join("models").join("u2net_human_seg.onnx")
        );
    }

    #[test]
    fn new_job_cancel_starts_false() {
        let job = spawn_analyze(vec![], 1);
        assert!(!job.cancel.load(Ordering::Relaxed));
        while job.rx.try_recv().is_ok() {}
    }
}
