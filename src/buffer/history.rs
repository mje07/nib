//! Linear undo/redo. A transaction groups every edit made by one normal-mode
//! command (including the whole insert session that may follow it).

use crate::types::Pos;

#[derive(Clone, Debug)]
pub struct Edit {
    /// Char index the edit happened at.
    pub at: usize,
    pub removed: String,
    pub inserted: String,
}

#[derive(Clone, Debug)]
pub struct Transaction {
    pub id: u64,
    pub edits: Vec<Edit>,
    pub cursor_before: Pos,
}

#[derive(Default, Debug)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    current: Option<Transaction>,
    next_id: u64,
    saved_id: u64,
}

impl History {
    pub fn new() -> Self {
        History { next_id: 1, ..Default::default() }
    }

    /// Open a transaction if none is open.
    pub fn begin(&mut self, cursor: Pos) {
        if self.current.is_none() {
            let id = self.next_id;
            self.next_id += 1;
            self.current = Some(Transaction { id, edits: Vec::new(), cursor_before: cursor });
        }
    }

    pub fn record(&mut self, edit: Edit, fallback_cursor: Pos) {
        self.begin(fallback_cursor);
        let cur = self.current.as_mut().unwrap();
        // Coalesce consecutive typing to keep transactions small.
        if let Some(last) = cur.edits.last_mut()
            && last.removed.is_empty()
            && edit.removed.is_empty()
            && last.at + last.inserted.chars().count() == edit.at
        {
            last.inserted.push_str(&edit.inserted);
            return;
        }
        cur.edits.push(edit);
    }

    /// Close the open transaction. Empty transactions are dropped.
    pub fn commit(&mut self) {
        if let Some(t) = self.current.take()
            && !t.edits.is_empty()
        {
            self.undo.push(t);
            self.redo.clear();
            if self.undo.len() > 2000 {
                self.undo.remove(0);
            }
        }
    }

    pub fn has_open_changes(&self) -> bool {
        self.current.as_ref().is_some_and(|t| !t.edits.is_empty())
    }

    pub fn pop_undo(&mut self) -> Option<Transaction> {
        self.commit();
        self.undo.pop()
    }
    pub fn push_redo(&mut self, t: Transaction) {
        self.redo.push(t);
    }
    pub fn pop_redo(&mut self) -> Option<Transaction> {
        self.commit();
        self.redo.pop()
    }
    pub fn push_undo_keep_redo(&mut self, t: Transaction) {
        self.undo.push(t);
    }

    fn top_id(&self) -> u64 {
        self.undo.last().map(|t| t.id).unwrap_or(0)
    }
    pub fn mark_saved(&mut self) {
        self.commit();
        self.saved_id = self.top_id();
    }
    pub fn is_modified(&self) -> bool {
        self.has_open_changes() || self.top_id() != self.saved_id
    }
    pub fn clear(&mut self) {
        *self = History::new();
    }
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }
}
