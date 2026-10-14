//! Command stack for undo and redo.
//!
//! A [`Command`] knows how to apply itself to a target and how to take itself back. [`History`]
//! owns the undo and redo stacks. The target type is generic so the scene, a single mesh in edit
//! mode or the UI state can each keep their own history.

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-undo";

/// One reversible edit on a `T`.
pub trait Command<T>: Send {
    /// Human-readable, e.g. "Move", shown in the Edit menu.
    fn name(&self) -> &str;
    fn apply(&mut self, target: &mut T);
    fn undo(&mut self, target: &mut T);
}

/// Snapshot command: remembers whole `before`/`after` states. Right for small targets; large
/// targets want a hand-written [`Command`].
pub struct Snapshot<T: Clone> {
    name: String,
    before: T,
    after: T,
}

impl<T: Clone> Snapshot<T> {
    /// `before` is the state as it is now; `edit` produces the new one.
    pub fn new(name: impl Into<String>, before: &T, edit: impl FnOnce(&mut T)) -> Self {
        let mut after = before.clone();
        edit(&mut after);
        Self { name: name.into(), before: before.clone(), after }
    }
}

impl<T: Clone + Send> Command<T> for Snapshot<T> {
    fn name(&self) -> &str {
        &self.name
    }
    fn apply(&mut self, target: &mut T) {
        *target = self.after.clone();
    }
    fn undo(&mut self, target: &mut T) {
        *target = self.before.clone();
    }
}

/// Undo/redo stacks. Pushing a command applies it and clears the redo stack.
pub struct History<T> {
    undo: Vec<Box<dyn Command<T>>>,
    redo: Vec<Box<dyn Command<T>>>,
    limit: usize,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self::new(256)
    }
}

impl<T> History<T> {
    pub fn new(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), limit: limit.max(1) }
    }

    /// Apply `cmd` to `target` and record it.
    pub fn push(&mut self, mut cmd: Box<dyn Command<T>>, target: &mut T) {
        cmd.apply(target);
        self.redo.clear();
        self.undo.push(cmd);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    /// Take back the newest command. Returns its name.
    pub fn undo(&mut self, target: &mut T) -> Option<String> {
        let mut cmd = self.undo.pop()?;
        cmd.undo(target);
        let name = cmd.name().to_owned();
        self.redo.push(cmd);
        Some(name)
    }

    /// Re-apply the newest undone command. Returns its name.
    pub fn redo(&mut self, target: &mut T) -> Option<String> {
        let mut cmd = self.redo.pop()?;
        cmd.apply(target);
        let name = cmd.name().to_owned();
        self.undo.push(cmd);
        Some(name)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    /// Name of what `undo` would take back.
    pub fn undo_name(&self) -> Option<&str> {
        self.undo.last().map(|c| c.name())
    }
    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|c| c.name())
    }
    pub fn len(&self) -> usize {
        self.undo.len()
    }
    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Add(i32);
    impl Command<i32> for Add {
        fn name(&self) -> &str {
            "Add"
        }
        fn apply(&mut self, t: &mut i32) {
            *t += self.0;
        }
        fn undo(&mut self, t: &mut i32) {
            *t -= self.0;
        }
    }

    #[test]
    fn crate_name() {
        assert_eq!(CRATE, "shape-undo");
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut h = History::default();
        let mut v = 0;
        h.push(Box::new(Add(2)), &mut v);
        h.push(Box::new(Add(3)), &mut v);
        assert_eq!(v, 5);
        assert_eq!(h.undo_name(), Some("Add"));
        assert_eq!(h.undo(&mut v), Some("Add".into()));
        assert_eq!(v, 2);
        assert!(h.can_redo());
        h.redo(&mut v);
        assert_eq!(v, 5);
        h.undo(&mut v);
        h.undo(&mut v);
        assert_eq!(v, 0);
        assert_eq!(h.undo(&mut v), None);
        h.redo(&mut v);
        assert_eq!(v, 2);
        h.push(Box::new(Add(10)), &mut v); // clears redo
        assert!(!h.can_redo());
        assert_eq!(v, 12);
    }

    #[test]
    fn limit_drops_oldest() {
        let mut h = History::new(2);
        let mut v = 0;
        for _ in 0..5 {
            h.push(Box::new(Add(1)), &mut v);
        }
        assert_eq!(h.len(), 2);
        while h.undo(&mut v).is_some() {}
        assert_eq!(v, 3);
    }

    #[test]
    fn snapshot_command() {
        let mut h = History::default();
        let mut s = String::from("a");
        let cmd = Snapshot::new("Append", &s, |t| t.push('b'));
        h.push(Box::new(cmd), &mut s);
        assert_eq!(s, "ab");
        h.undo(&mut s);
        assert_eq!(s, "a");
    }
}
