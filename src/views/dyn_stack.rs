use std::{
    hash::{BuildHasherDefault, Hash},
    marker::PhantomData,
};

use floem_reactive::{as_child_of_current_scope, create_effect, Scope};
use rustc_hash::FxHasher;
use smallvec::SmallVec;

use crate::{
    context::{AppState, UpdateCx},
    id::Id,
    view::{view_children_set_parent_id, View, ViewData},
};

pub(crate) type FxIndexSet<T> = indexmap::IndexSet<T, BuildHasherDefault<FxHasher>>;

#[derive(educe::Educe)]
#[educe(Debug)]
pub(crate) struct HashRun<T>(#[educe(Debug(ignore))] pub(crate) T);

pub struct DynStack<V, T>
where
    V: View,
    T: 'static,
{
    data: ViewData,
    children: Vec<Option<(V, Scope)>>,
    view_fn: Box<dyn Fn(T) -> (V, Scope)>,
    phantom: PhantomData<T>,
}

pub fn dyn_stack<IF, I, T, KF, K, VF, V>(each_fn: IF, key_fn: KF, view_fn: VF) -> DynStack<V, T>
where
    IF: Fn() -> I + 'static,
    I: IntoIterator<Item = T>,
    KF: Fn(&T) -> K + 'static,
    K: Eq + Hash + 'static,
    VF: Fn(T) -> V + 'static,
    V: View + 'static,
    T: 'static,
{
    let id = Id::next();
    create_effect(move |prev_hash_run| {
        let items = each_fn();
        let items = items.into_iter().collect::<SmallVec<[_; 128]>>();
        let hashed_items = items.iter().map(&key_fn).collect::<FxIndexSet<_>>();
        let diff = if let Some(HashRun(prev_hash_run)) = prev_hash_run {
            let mut cmds = diff(&prev_hash_run, &hashed_items);
            let mut items = items
                .into_iter()
                .map(|i| Some(i))
                .collect::<SmallVec<[Option<_>; 128]>>();
            for added in &mut cmds.added {
                added.view = Some(items[added.at].take().unwrap());
            }
            cmds
        } else {
            let mut diff = Diff::default();
            for (i, item) in each_fn().into_iter().enumerate() {
                diff.added.push(DiffOpAdd {
                    at: i,
                    view: Some(item),
                });
            }
            diff
        };
        id.update_state(diff);
        HashRun(hashed_items)
    });
    let view_fn = Box::new(as_child_of_current_scope(view_fn));
    DynStack {
        data: ViewData::new(id),
        children: Vec::new(),
        view_fn,
        phantom: PhantomData,
    }
}

impl<V: View + 'static, T> View for DynStack<V, T> {
    fn view_data(&self) -> &ViewData {
        &self.data
    }

    fn view_data_mut(&mut self) -> &mut ViewData {
        &mut self.data
    }

    fn for_each_child<'a>(&'a self, for_each: &mut dyn FnMut(&'a dyn View) -> bool) {
        for child in self.children.iter().filter_map(|child| child.as_ref()) {
            if for_each(&child.0) {
                break;
            }
        }
    }

    fn for_each_child_mut<'a>(&'a mut self, for_each: &mut dyn FnMut(&'a mut dyn View) -> bool) {
        for child in self.children.iter_mut().filter_map(|child| child.as_mut()) {
            if for_each(&mut child.0) {
                break;
            }
        }
    }

    fn for_each_child_rev_mut<'a>(
        &'a mut self,
        for_each: &mut dyn FnMut(&'a mut dyn View) -> bool,
    ) {
        for child in self
            .children
            .iter_mut()
            .rev()
            .filter_map(|child| child.as_mut())
        {
            if for_each(&mut child.0) {
                break;
            }
        }
    }

    fn debug_name(&self) -> std::borrow::Cow<'static, str> {
        "DynStack".into()
    }

    fn update(&mut self, cx: &mut UpdateCx, state: Box<dyn std::any::Any>) {
        if let Ok(diff) = state.downcast() {
            apply_diff(
                self.id(),
                cx.app_state,
                *diff,
                &mut self.children,
                &self.view_fn,
            );
            cx.request_all(self.id());
        }
    }
}

#[derive(Debug)]
pub struct Diff<V> {
    pub(crate) removed: SmallVec<[DiffOpRemove; 8]>,
    pub(crate) moved: SmallVec<[DiffOpMove; 8]>,
    pub(crate) added: SmallVec<[DiffOpAdd<V>; 8]>,
    pub(crate) clear: bool,
}

impl<V> Default for Diff<V> {
    fn default() -> Self {
        Self {
            removed: Default::default(),
            moved: Default::default(),
            added: Default::default(),
            clear: false,
        }
    }
}

impl<V> Diff<V> {
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.moved.is_empty() && self.added.is_empty() && !self.clear
    }
}

#[derive(Debug)]
pub(crate) struct DiffOpMove {
    from: usize,
    to: usize,
}

#[derive(Debug)]
pub(crate) struct DiffOpAdd<V> {
    pub(crate) at: usize,
    pub(crate) view: Option<V>,
}

#[derive(Debug)]
pub(crate) struct DiffOpRemove {
    at: usize,
}

/// Calculates the operations need to get from `a` to `b`.
pub(crate) fn diff<K: Eq + Hash, V>(from: &FxIndexSet<K>, to: &FxIndexSet<K>) -> Diff<V> {
    if from.is_empty() && to.is_empty() {
        return Diff::default();
    } else if to.is_empty() {
        return Diff {
            clear: true,
            ..Default::default()
        };
    }

    // Get removed items
    let mut removed = from.difference(to);

    let removed_cmds = removed
        .clone()
        .map(|k| from.get_full(k).unwrap().0)
        .map(|idx| DiffOpRemove { at: idx });

    // Get added items
    let mut added = to.difference(from);

    let added_cmds = added
        .clone()
        .map(|k| to.get_full(k).unwrap().0)
        .map(|idx| DiffOpAdd {
            at: idx,
            view: None,
        });

    // Get moved items
    let mut normalized_idx = 0;
    let mut move_cmds = SmallVec::<[_; 8]>::with_capacity(to.len());
    let mut added_idx = added.next().map(|k| to.get_full(k).unwrap().0);
    let mut removed_idx = removed.next().map(|k| from.get_full(k).unwrap().0);

    for (idx, k) in to.iter().enumerate() {
        if let Some(added_idx) = added_idx.as_mut().filter(|r_i| **r_i == idx) {
            if let Some(next_added) = added.next().map(|k| to.get_full(k).unwrap().0) {
                *added_idx = next_added;

                normalized_idx = usize::wrapping_sub(normalized_idx, 1);
            }
        }

        if let Some(removed_idx) = removed_idx.as_mut().filter(|r_i| **r_i == idx) {
            normalized_idx = normalized_idx.wrapping_add(1);

            if let Some(next_removed) = removed.next().map(|k| from.get_full(k).unwrap().0) {
                *removed_idx = next_removed;
            }
        }

        if let Some((from_idx, _)) = from.get_full(k) {
            if from_idx != normalized_idx || from_idx != idx {
                move_cmds.push(DiffOpMove {
                    from: from_idx,
                    to: idx,
                });
            }
        }

        normalized_idx = normalized_idx.wrapping_add(1);
    }

    let mut diffs = Diff {
        removed: removed_cmds.collect(),
        moved: move_cmds,
        added: added_cmds.collect(),
        clear: false,
    };

    if !from.is_empty()
        && !to.is_empty()
        && diffs.removed.len() == from.len()
        && diffs.moved.is_empty()
    {
        diffs.clear = true;
    }

    diffs
}

fn remove_index<V: View>(
    app_state: &mut AppState,
    children: &mut [Option<(V, Scope)>],
    index: usize,
) -> Option<()> {
    let (mut view, scope) = std::mem::take(&mut children[index])?;
    app_state.remove_view(&mut view);
    scope.dispose();
    Some(())
}

pub(super) fn apply_diff<T, V, VF>(
    view_id: Id,
    app_state: &mut AppState,
    mut diff: Diff<T>,
    children: &mut Vec<Option<(V, Scope)>>,
    view_fn: &VF,
) where
    V: View,
    VF: Fn(T) -> (V, Scope),
{
    // Resize children if needed
    if diff.added.len().checked_sub(diff.removed.len()).is_some() {
        let target_size =
            children.len() + (diff.added.len() as isize - diff.removed.len() as isize) as usize;

        children.resize_with(target_size, || None);
    }

    // We need to hold a list of items which will be moved, and
    // we can only perform the move after all commands have run, otherwise,
    // we risk overwriting one of the values
    let mut items_to_move = Vec::with_capacity(diff.moved.len());

    // The order of cmds needs to be:
    // 1. Clear
    // 2. Removed
    // 3. Moved
    // 4. Add
    if diff.clear {
        for i in 0..children.len() {
            remove_index(app_state, children, i);
        }
        diff.removed.clear();
    }

    for DiffOpRemove { at } in diff.removed {
        remove_index(app_state, children, at);
    }

    for DiffOpMove { from, to } in diff.moved {
        let item = children[from].take().unwrap();
        items_to_move.push((to, item));
    }

    for DiffOpAdd { at, view } in diff.added {
        children[at] = view.map(view_fn);
        if let Some((child, _)) = children[at].as_ref() {
            child.id().set_parent(view_id);
            view_children_set_parent_id(child);
        }
    }

    for (to, each_item) in items_to_move {
        children[to] = Some(each_item);
    }

    // Now, remove the holes that might have been left from removing
    // items
    children.retain(|c| c.is_some());
}
