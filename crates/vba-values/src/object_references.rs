use crate::{ObjectRef, ScalarKind, Value, VbaArray};
use std::collections::{BTreeMap, BTreeSet};

/// Counts registered objects in locations and in the executor's explicit roots.
/// Opaque host objects and unregistered tokens have no ownership in this store.
#[derive(Default)]
pub struct ObjectReferences {
    counts: BTreeMap<ObjectRef, usize>,
    unreferenced: BTreeSet<ObjectRef>,
}

impl ObjectReferences {
    pub fn register(&mut self, object: ObjectRef) {
        self.counts.insert(object, 0);
        self.unreferenced.insert(object);
    }

    pub fn unregister(&mut self, object: ObjectRef) {
        self.counts.remove(&object);
        self.unreferenced.remove(&object);
    }

    pub fn retain(&mut self, object: ObjectRef) {
        if let Some(count) = self.counts.get_mut(&object) {
            if *count == 0 {
                self.unreferenced.remove(&object);
            }
            *count += 1;
        }
    }

    pub fn release(&mut self, object: ObjectRef) {
        if let Some(count) = self.counts.get_mut(&object)
            && let Some(remaining) = count.checked_sub(1)
        {
            *count = remaining;
            if remaining == 0 {
                self.unreferenced.insert(object);
            }
        }
    }

    pub fn next_unreferenced(&self) -> Option<ObjectRef> {
        self.unreferenced.first().copied()
    }

    pub fn retain_value(&mut self, value: &Value) {
        if !self.counts.is_empty() {
            for object in references_in_value(value) {
                self.retain(object);
            }
        }
    }

    pub fn release_value(&mut self, value: &Value) {
        if !self.counts.is_empty() {
            for object in references_in_value(value) {
                self.release(object);
            }
        }
    }

    pub fn retain_array(&mut self, array: &VbaArray) {
        if !self.counts.is_empty() {
            for object in reference_elements(array).iter().filter_map(as_object) {
                self.retain(object);
            }
        }
    }

    pub fn release_array(&mut self, array: &VbaArray) {
        if !self.counts.is_empty() {
            for object in reference_elements(array).iter().filter_map(as_object) {
                self.release(object);
            }
        }
    }
}

fn as_object(value: &Value) -> Option<ObjectRef> {
    match value {
        Value::Object(object) => Some(*object),
        _ => None,
    }
}

fn references_in_value(value: &Value) -> impl Iterator<Item = ObjectRef> + '_ {
    let values = match value {
        Value::Array(array) => reference_elements(array),
        value => std::slice::from_ref(value),
    };
    values.iter().filter_map(as_object)
}

fn reference_elements(array: &VbaArray) -> &[Value] {
    match array.kind() {
        ScalarKind::Object | ScalarKind::Variant => array.elements(),
        _ => &[],
    }
}
