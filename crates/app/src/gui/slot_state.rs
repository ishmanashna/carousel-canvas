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

    pub fn len(&self) -> usize {
        self.assignments.len()
    }

    /// Truncate or pad with `None`. Does not checkpoint. Does not clear undo.
    pub fn resize_to(&mut self, n: usize) {
        if n < self.assignments.len() {
            self.assignments.truncate(n);
        } else if n > self.assignments.len() {
            self.assignments.resize_with(n, || None);
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
        self.resize_to(paths.len());
        for (i, p) in paths.into_iter().enumerate() {
            self.assignments[i] = p.map(SlotFill::new);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn resize_to_smaller_truncates_larger_pads_none() {
        let mut state = SlotState::new();
        state.assignments[0] = Some(SlotFill::new(path("a.jpg")));
        state.assignments[1] = Some(SlotFill::new(path("b.jpg")));
        state.assignments[2] = Some(SlotFill::new(path("c.jpg")));

        state.resize_to(2);
        assert_eq!(state.len(), 2);
        assert!(state.assignments[0].is_some());
        assert!(state.assignments[1].is_some());

        state.resize_to(4);
        assert_eq!(state.len(), 4);
        assert!(state.assignments[0].is_some());
        assert!(state.assignments[1].is_some());
        assert!(state.assignments[2].is_none());
        assert!(state.assignments[3].is_none());
    }

    #[test]
    fn set_assignments_from_paths_resizes_to_path_count() {
        let mut state = SlotState::new();
        state.set_assignments_from_paths(vec![
            Some(path("one.jpg")),
            Some(path("two.jpg")),
            None,
        ]);
        assert_eq!(state.len(), 3);
        assert_eq!(
            state.assignments[0].as_ref().unwrap().path,
            path("one.jpg")
        );
        assert_eq!(
            state.assignments[1].as_ref().unwrap().path,
            path("two.jpg")
        );
        assert!(state.assignments[2].is_none());
    }

    #[test]
    fn assign_after_shrink_ignores_out_of_range_slot() {
        let mut state = SlotState::new();
        state.assignments[0] = Some(SlotFill::new(path("keep.jpg")));
        state.assignments[1] = Some(SlotFill::new(path("also.jpg")));
        state.resize_to(2);

        state.assign(5, path("ignored.jpg"));

        assert_eq!(state.len(), 2);
        assert_eq!(
            state.assignments[0].as_ref().unwrap().path,
            path("keep.jpg")
        );
        assert_eq!(
            state.assignments[1].as_ref().unwrap().path,
            path("also.jpg")
        );
    }

    #[test]
    fn undo_restores_length_after_resize() {
        let mut state = SlotState::new();
        state.assignments[0] = Some(SlotFill::new(path("a.jpg")));
        state.assignments[1] = Some(SlotFill::new(path("b.jpg")));
        state.assignments[2] = Some(SlotFill::new(path("c.jpg")));
        state.resize_to(3);
        state.checkpoint();

        state.resize_to(1);
        assert_eq!(state.len(), 1);

        state.undo(None);
        assert_eq!(state.len(), 3);
        assert_eq!(
            state.assignments[0].as_ref().unwrap().path,
            path("a.jpg")
        );
        assert_eq!(
            state.assignments[1].as_ref().unwrap().path,
            path("b.jpg")
        );
        assert_eq!(
            state.assignments[2].as_ref().unwrap().path,
            path("c.jpg")
        );
    }
}
