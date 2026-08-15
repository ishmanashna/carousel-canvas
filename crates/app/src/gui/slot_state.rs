use std::path::PathBuf;

use core::{max_strip_slots, SlotFill};

pub const UNDO_MAX: usize = 50;

#[derive(Debug, Clone)]
pub struct SlotState {
    pub assignments: Vec<Option<SlotFill>>,
    undo: Vec<Vec<Option<SlotFill>>>,
    redo: Vec<Vec<Option<SlotFill>>>,
}

impl SlotState {
    pub fn new() -> Self {
        Self {
            assignments: vec![None; max_strip_slots()],
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.assignments = vec![None; max_strip_slots()];
        self.clear_undo();
    }

    pub fn clear_undo(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn clone_assignments(&self) -> Vec<Option<SlotFill>> {
        self.assignments.clone()
    }

    pub fn checkpoint(&mut self) {
        self.undo.push(self.clone_assignments());
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> bool {
        if self.undo.is_empty() {
            return false;
        }
        self.redo.push(self.clone_assignments());
        self.assignments = self.undo.pop().unwrap();
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.redo.is_empty() {
            return false;
        }
        self.undo.push(self.clone_assignments());
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
        self.assignments = self.redo.pop().unwrap();
        true
    }

    pub fn assign(&mut self, slot: usize, path: PathBuf) {
        if slot >= self.assignments.len() {
            return;
        }
        self.checkpoint();
        self.assignments[slot] = Some(SlotFill::new(path));
    }

    pub fn clear_slot(&mut self, slot: usize) {
        if slot >= self.assignments.len() {
            return;
        }
        self.checkpoint();
        self.assignments[slot] = None;
    }

    pub fn swap_slots(&mut self, a: usize, b: usize) {
        if a == b || a >= self.assignments.len() || b >= self.assignments.len() {
            return;
        }
        self.checkpoint();
        self.assignments.swap(a, b);
    }

    pub fn set_assignments_from_paths(&mut self, paths: Vec<Option<PathBuf>>) {
        for (i, p) in paths.into_iter().enumerate() {
            if i < self.assignments.len() {
                self.assignments[i] = p.map(SlotFill::new);
            }
        }
    }
}
