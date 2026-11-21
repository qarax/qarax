//! The VMs a backend manages, by id.

use std::collections::HashMap;
use std::sync::Mutex;

/// A backend's VMs, by id.
///
/// An add of a VM (a create, a restore, a fork, a migration in) finds its id
/// free, then takes many steps (TAPs, disks, a VMM process named for the id)
/// before it has the VM. So it reserves the id first (see [`Vms::reserve`]):
/// until the add ends, other adds of the id are refused, and reads find no
/// VM by that id, as they found none before the add began.
///
/// Locked only to find, add, remove or list VMs, never across an await: a std
/// Mutex, which the Send futures here can't hold across one.
pub(crate) struct Vms<T>(Mutex<HashMap<String, Entry<T>>>);

enum Entry<T> {
    /// Reserved by an [`Adding`].
    Adding,
    Added(T),
}

impl<T> Default for Vms<T> {
    fn default() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

impl<T> Vms<T> {
    /// `read` of the VM `id`, if there is one.
    pub(crate) fn get<R>(&self, id: &str, read: impl FnOnce(&T) -> R) -> Option<R> {
        match self.0.lock().unwrap().get(id) {
            Some(Entry::Added(vm)) => Some(read(vm)),
            Some(Entry::Adding) | None => None,
        }
    }

    /// `read` of each VM.
    pub(crate) fn list<R>(&self, mut read: impl FnMut(&T) -> R) -> Vec<R> {
        self.0
            .lock()
            .unwrap()
            .values()
            .filter_map(|entry| match entry {
                Entry::Added(vm) => Some(read(vm)),
                Entry::Adding => None,
            })
            .collect()
    }

    /// Reserve `ids` for VMs about to be added, or return one of them that
    /// is a VM's or another add's, reserving none. Dropping the reservation
    /// before it adds the VMs, because the add failed or was cancelled, frees
    /// the ids.
    pub(crate) fn reserve(&self, ids: Vec<String>) -> Result<Adding<'_, T>, String> {
        let mut entries = self.0.lock().unwrap();
        if let Some(taken) = ids.iter().find(|id| entries.contains_key(*id)) {
            return Err(taken.clone());
        }
        for id in &ids {
            entries.insert(id.clone(), Entry::Adding);
        }
        Ok(Adding { vms: self, ids })
    }

    /// Add a VM found at startup, before the node takes requests: nothing
    /// else adds VMs then.
    pub(crate) fn insert(&self, id: String, vm: T) {
        self.0.lock().unwrap().insert(id, Entry::Added(vm));
    }

    /// Take out the VM `id`, whose instance the caller has taken.
    pub(crate) fn remove(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }
}

/// Ids reserved for VMs being added (see [`Vms::reserve`]).
pub(crate) struct Adding<'a, T> {
    vms: &'a Vms<T>,
    ids: Vec<String>,
}

impl<T> Adding<'_, T> {
    /// Add `vms`, the VMs the ids were reserved for, by their ids.
    pub(crate) fn finish(mut self, vms: impl IntoIterator<Item = (String, T)>) {
        let mut entries = self.vms.0.lock().unwrap();
        for (id, vm) in vms {
            entries.insert(id, Entry::Added(vm));
        }
        // Added: nothing is left for dropping this to free.
        self.ids.clear();
    }
}

impl<T> Drop for Adding<'_, T> {
    fn drop(&mut self) {
        // Ids not added: the add failed or was cancelled. Their entries are
        // still this reservation's, as nothing else touches a reserved id's.
        let mut entries = self.vms.0.lock().unwrap();
        for id in &self.ids {
            entries.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fork reserves its VMs' ids all at once: one of them taken reserves
    /// none of the others.
    #[test]
    fn ids_are_reserved_all_or_none() {
        let vms = Vms::<()>::default();
        let _adding = vms.reserve(vec!["b".to_string()]).unwrap();
        assert_eq!(
            vms.reserve(vec!["a".to_string(), "b".to_string()]).err(),
            Some("b".to_string())
        );
        vms.reserve(vec!["a".to_string()]).unwrap();
    }
}
