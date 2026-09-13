use std::path::PathBuf;

use core::{max_strip_slots, SlotFill, StripSlotDef};

pub const UNDO_MAX: usize = 50;

#[derive(Debug, Clone)]
struct UndoFrame {
    assignments: Vec<Option<SlotFill>>,
    slot_geometry: Option<Vec<StripSlotDef>>,
}

#[derive(Debug, Clone)]
pub struct UndoRestore {
    pub slot_geometry: Option<Vec<StripSlotDef>>,
}

#[derive(Debug, Clone)]
pub struct SlotState {
    pub assignments: Vec<Option<SlotFill>>,
    undo: Vec<UndoFrame>,
    redo: Vec<UndoFrame>,
    /// Geometry captured on the next `checkpoint` (set by GUI when layout is locked).
    pending_geometry: Option<Vec<StripSlotDef>>,
}

impl SlotState {
    pub fn new() -> Self {
        Self {
            assignments: vec![None; max_strip_slots()],
            undo: Vec::new(),
            redo: Vec::new(),
            pending_geometry: None,
        }
    }

    pub fn reset(&mut self) {
        self.assignments = vec![None; max_strip_slots()];
        self.clear_undo();
    }

    pub fn clear_undo(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending_geometry = None;
    }

    pub fn set_pending_geometry(&mut self, geometry: Option<Vec<StripSlotDef>>) {
        self.pending_geometry = geometry;
    }

    fn clone_assignments(&self) -> Vec<Option<SlotFill>> {
        self.assignments.clone()
    }

    pub fn checkpoint(&mut self) {
        self.undo.push(UndoFrame {
            assignments: self.clone_assignments(),
            slot_geometry: self.pending_geometry.clone(),
        });
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current_geometry: Option<Vec<StripSlotDef>>) -> Option<UndoRestore> {
        if self.undo.is_empty() {
            return None;
        }
        self.redo.push(UndoFrame {
            assignments: self.clone_assignments(),
            slot_geometry: current_geometry,
        });
        if self.redo.len() > UNDO_MAX {
            self.redo.remove(0);
        }
        let frame = self.undo.pop().unwrap();
        self.assignments = frame.assignments;
        Some(UndoRestore {
            slot_geometry: frame.slot_geometry,
        })
    }

    pub fn redo(&mut self, current_geometry: Option<Vec<StripSlotDef>>) -> Option<UndoRestore> {
        if self.redo.is_empty() {
            return None;
        }
        self.undo.push(UndoFrame {
            assignments: self.clone_assignments(),
            slot_geometry: current_geometry,
        });
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
        let frame = self.redo.pop().unwrap();
        self.assignments = frame.assignments;
        Some(UndoRestore {
            slot_geometry: frame.slot_geometry,
        })
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
