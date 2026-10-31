//! Undo/redo over whole-document snapshots. Graphics are shared via `Arc`,
//! so a snapshot is cheap.

use super::Document;

pub struct History {
    undo: Vec<Document>,
    redo: Vec<Document>,
    last_tag: Option<String>,
}

impl History {
    pub fn new() -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), last_tag: None }
    }

    /// Call before changing `doc`.
    pub fn checkpoint(&mut self, doc: &Document) {
        self.last_tag = None;
        self.undo.push(doc.clone());
        self.redo.clear();
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
    }

    /// Like `checkpoint`, but consecutive calls with the same tag (e.g. while
    /// dragging a colour picker or slider) make a single undo step.
    pub fn checkpoint_tagged(&mut self, doc: &Document, tag: &str) {
        if self.last_tag.as_deref() != Some(tag) {
            self.checkpoint(doc);
            self.last_tag = Some(tag.to_string());
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// How many undo steps there are.
    pub fn depth(&self) -> usize {
        self.undo.len()
    }

    /// Merges every undo step taken since the history had `depth` steps
    /// into one (the state before the first of them).
    pub fn squash_since(&mut self, depth: usize) {
        self.undo.truncate(depth + 1);
    }

    pub fn undo(&mut self, doc: &mut Document) -> bool {
        self.last_tag = None;
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(doc, prev));
        true
    }

    pub fn redo(&mut self, doc: &mut Document) -> bool {
        self.last_tag = None;
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(doc, next));
        true
    }
}
