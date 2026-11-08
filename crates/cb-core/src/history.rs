//! Command based undo/redo. Each command stores only the items it touched,
//! instead of the legacy whole-page snapshots.

use crate::doc::{Document, Item, Page, Uid};

#[derive(Debug, Clone)]
pub enum Command {
    /// Items added (z-index recorded so redo restores the same order).
    Add { page: usize, items: Vec<(usize, Item)> },
    /// Items removed with their original z-index.
    Remove { page: usize, items: Vec<(usize, Item)> },
    /// Items changed in place (move / recolor / text edit): before & after share uids.
    Modify { page: usize, before: Vec<Item>, after: Vec<Item> },
    /// Strokes split by the eraser: removed originals and their fragments.
    Replace { page: usize, removed: Vec<(usize, Item)>, added: Vec<(usize, Item)> },
    InsertPage { index: usize, page: Page },
    DeletePage { index: usize, page: Page },
    /// Whole page content cleared.
    ClearPage { index: usize, page: Page },
    Group(Vec<Command>),
}

impl Command {
    pub fn page(&self) -> Option<usize> {
        match self {
            Command::Add { page, .. } | Command::Remove { page, .. } | Command::Modify { page, .. } | Command::Replace { page, .. } => Some(*page),
            Command::InsertPage { index, .. } | Command::DeletePage { index, .. } | Command::ClearPage { index, .. } => Some(*index),
            Command::Group(v) => v.first().and_then(|c| c.page()),
        }
    }

    fn apply(&self, doc: &mut Document) {
        match self {
            Command::Add { page, items } => {
                for (i, it) in items {
                    doc.insert_item(*page, *i, it.clone());
                }
            }
            Command::Remove { page, items } => {
                for (_, it) in items.iter().rev() {
                    doc.remove_item(*page, it.uid());
                }
            }
            Command::Modify { page, after, .. } => {
                for it in after {
                    doc.replace_item(*page, it.clone());
                }
            }
            Command::Replace { page, removed, added } => {
                for (_, it) in removed.iter().rev() {
                    doc.remove_item(*page, it.uid());
                }
                for (i, it) in added {
                    doc.insert_item(*page, *i, it.clone());
                }
            }
            Command::InsertPage { index, page } => {
                let i = (*index).min(doc.pages.len());
                doc.pages.insert(i, page.clone());
            }
            Command::DeletePage { index, .. } => {
                if *index < doc.pages.len() {
                    doc.pages.remove(*index);
                }
                if doc.pages.is_empty() {
                    doc.pages.push(Page::default());
                }
            }
            Command::ClearPage { index, .. } => {
                if let Some(p) = doc.pages.get_mut(*index) {
                    p.strokes.clear();
                    p.texts.clear();
                    p.images.clear();
                }
            }
            Command::Group(v) => {
                for c in v {
                    c.apply(doc)
                }
            }
        }
        doc.touch();
    }

    fn revert(&self, doc: &mut Document) {
        match self {
            Command::Add { page, items } => {
                for (_, it) in items.iter().rev() {
                    doc.remove_item(*page, it.uid());
                }
            }
            Command::Remove { page, items } => {
                for (i, it) in items {
                    doc.insert_item(*page, *i, it.clone());
                }
            }
            Command::Modify { page, before, .. } => {
                for it in before {
                    doc.replace_item(*page, it.clone());
                }
            }
            Command::Replace { page, removed, added } => {
                for (_, it) in added.iter().rev() {
                    doc.remove_item(*page, it.uid());
                }
                for (i, it) in removed {
                    doc.insert_item(*page, *i, it.clone());
                }
            }
            Command::InsertPage { index, .. } => {
                if *index < doc.pages.len() {
                    doc.pages.remove(*index);
                }
            }
            Command::DeletePage { index, page } => {
                // A delete of the only page left a fresh blank page; drop it.
                if doc.pages.len() == 1 && doc.pages[0].is_empty() && *index == 0 {
                    doc.pages.clear();
                }
                let i = (*index).min(doc.pages.len());
                doc.pages.insert(i, page.clone());
            }
            Command::ClearPage { index, page } => {
                if let Some(p) = doc.pages.get_mut(*index) {
                    p.strokes = page.strokes.clone();
                    p.texts = page.texts.clone();
                    p.images = page.images.clone();
                }
            }
            Command::Group(v) => {
                for c in v.iter().rev() {
                    c.revert(doc)
                }
            }
        }
        doc.touch();
    }
}

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Command>,
    redo: Vec<Command>,
    /// Merge window for consecutive pen strokes (legacy: 450 ms).
    merge_open: bool,
    pub limit: usize,
    /// Incremented whenever history changes; used for "unsaved changes".
    pub version: u64,
}

impl History {
    pub fn new() -> History {
        History { limit: 500, ..Default::default() }
    }

    /// Apply `cmd` to the document and record it.
    pub fn execute(&mut self, doc: &mut Document, cmd: Command) {
        cmd.apply(doc);
        self.record(cmd);
    }

    /// Record a command whose effect is already applied to the document.
    pub fn record(&mut self, cmd: Command) {
        self.redo.clear();
        self.merge_open = false;
        self.undo.push(cmd);
        if self.undo.len() > self.limit.max(1) {
            self.undo.remove(0);
        }
        self.version += 1;
    }

    /// Record an added stroke, merging with the previous stroke when `merge` is set
    /// and the previous command was a mergeable stroke add on the same page.
    pub fn record_stroke(&mut self, page: usize, idx: usize, item: Item, merge: bool) {
        if merge && self.merge_open {
            if let Some(Command::Add { page: p, items }) = self.undo.last_mut() {
                if *p == page {
                    items.push((idx, item));
                    self.version += 1;
                    return;
                }
            }
        }
        self.record(Command::Add { page, items: vec![(idx, item)] });
        self.merge_open = true;
    }

    pub fn break_merge(&mut self) {
        self.merge_open = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self, doc: &mut Document) -> Option<usize> {
        let c = self.undo.pop()?;
        c.revert(doc);
        let p = c.page();
        self.redo.push(c);
        self.merge_open = false;
        self.version += 1;
        p
    }

    pub fn redo(&mut self, doc: &mut Document) -> Option<usize> {
        let c = self.redo.pop()?;
        c.apply(doc);
        let p = c.page();
        self.undo.push(c);
        self.merge_open = false;
        self.version += 1;
        p
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.merge_open = false;
        self.version += 1;
    }
}

/// Helper: build a Remove command for the given uids (skips unknown uids).
pub fn remove_command(doc: &Document, page: usize, uids: &[Uid]) -> Command {
    let mut items: Vec<(usize, Item)> = Vec::new();
    // Record in ascending index order per kind so revert re-inserts correctly.
    let mut found: Vec<(crate::doc::ItemKind, usize, Item)> = uids
        .iter()
        .filter_map(|u| {
            let (k, i) = doc.find(page, *u)?;
            Some((k, i, doc.get_item(page, *u)?))
        })
        .collect();
    found.sort_by_key(|(k, i, _)| (*k as u8, *i));
    // When removing in reverse order the stored indices stay valid; when
    // re-inserting in forward order they are restored exactly.
    for (_, i, it) in found {
        items.push((i, it));
    }
    Command::Remove { page, items }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Color, Stroke};

    fn stroke(doc: &mut Document) -> Item {
        let u = doc.alloc_uid();
        Item::Stroke(Stroke::new(u, Color::rgb(0, 0, 0), 3.0))
    }

    #[test]
    fn add_undo_redo() {
        let mut d = Document::new();
        let mut h = History::new();
        let s = stroke(&mut d);
        h.execute(&mut d, Command::Add { page: 0, items: vec![(0, s)] });
        assert_eq!(d.pages[0].strokes.len(), 1);
        h.undo(&mut d);
        assert_eq!(d.pages[0].strokes.len(), 0);
        h.redo(&mut d);
        assert_eq!(d.pages[0].strokes.len(), 1);
    }

    #[test]
    fn remove_restores_order() {
        let mut d = Document::new();
        let mut h = History::new();
        let items: Vec<Item> = (0..5).map(|_| stroke(&mut d)).collect();
        for (i, it) in items.iter().enumerate() {
            d.insert_item(0, i, it.clone());
        }
        let order: Vec<u64> = d.pages[0].strokes.iter().map(|s| s.uid).collect();
        let cmd = remove_command(&d, 0, &[order[1], order[3]]);
        h.execute(&mut d, cmd);
        assert_eq!(d.pages[0].strokes.len(), 3);
        h.undo(&mut d);
        let after: Vec<u64> = d.pages[0].strokes.iter().map(|s| s.uid).collect();
        assert_eq!(order, after);
    }

    #[test]
    fn merge_strokes() {
        let mut d = Document::new();
        let mut h = History::new();
        for i in 0..3 {
            let s = stroke(&mut d);
            d.insert_item(0, i, s.clone());
            h.record_stroke(0, i, s, true);
        }
        h.undo(&mut d);
        assert!(d.pages[0].strokes.is_empty());
    }

    #[test]
    fn delete_only_page() {
        let mut d = Document::new();
        let mut h = History::new();
        let s = stroke(&mut d);
        d.insert_item(0, 0, s);
        let p = d.pages[0].clone();
        h.execute(&mut d, Command::DeletePage { index: 0, page: p });
        assert_eq!(d.pages.len(), 1);
        assert!(d.pages[0].is_empty());
        h.undo(&mut d);
        assert_eq!(d.pages.len(), 1);
        assert_eq!(d.pages[0].strokes.len(), 1);
    }
}
