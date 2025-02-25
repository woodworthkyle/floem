use super::{v_stack_from_iter, Decorators, Stack};
use crate::context::StyleCx;
use crate::reactive::create_effect;
use crate::style::Style;
use crate::EventPropagation;
use crate::{
    event::{Event, EventListener},
    id::Id,
    keyboard::{Key, NamedKey},
    view::{View, ViewData},
};
use floem_reactive::{create_rw_signal, RwSignal};

enum ListUpdate {
    SelectionChanged,
    ScrollToSelected,
}

pub(crate) struct Item {
    pub(crate) data: ViewData,
    pub(crate) index: usize,
    pub(crate) selection: RwSignal<Option<usize>>,
    pub(crate) child: Box<dyn View>,
}

pub struct List {
    data: ViewData,
    selection: RwSignal<Option<usize>>,
    child: Stack,
}

impl List {
    pub fn selection(&self) -> RwSignal<Option<usize>> {
        self.selection
    }

    pub fn on_select(self, on_select: impl Fn(Option<usize>) + 'static) -> Self {
        create_effect(move |_| {
            let selection = self.selection.get();
            on_select(selection);
        });
        self
    }
}

pub fn list<V>(iterator: impl IntoIterator<Item = V>) -> List
where
    V: View + 'static,
{
    let id = Id::next();
    let selection = create_rw_signal(None);
    create_effect(move |_| {
        selection.track();
        id.update_state(ListUpdate::SelectionChanged);
    });
    let stack = v_stack_from_iter(iterator.into_iter().enumerate().map(move |(index, v)| {
        Item {
            data: ViewData::new(Id::next()),
            selection,
            index,
            child: Box::new(v),
        }
        .on_click_stop(move |_| {
            if selection.get_untracked() != Some(index) {
                selection.set(Some(index))
            }
        })
    }))
    .style(|s| s.width_full().height_full());
    let length = stack.children.len();
    List {
        data: ViewData::new(id),
        selection,
        child: stack,
    }
    .keyboard_navigatable()
    .on_event(EventListener::KeyDown, move |e| {
        if let Event::KeyDown(key_event) = e {
            match key_event.key.logical_key {
                Key::Named(NamedKey::Home) => {
                    if length > 0 {
                        selection.set(Some(0));
                        id.update_state(ListUpdate::ScrollToSelected);
                    }
                    EventPropagation::Stop
                }
                Key::Named(NamedKey::End) => {
                    if length > 0 {
                        selection.set(Some(length - 1));
                        id.update_state(ListUpdate::ScrollToSelected);
                    }
                    EventPropagation::Stop
                }
                Key::Named(NamedKey::ArrowUp) => {
                    let current = selection.get_untracked();
                    match current {
                        Some(i) => {
                            if i > 0 {
                                selection.set(Some(i - 1));
                                id.update_state(ListUpdate::ScrollToSelected);
                            }
                        }
                        None => {
                            if length > 0 {
                                selection.set(Some(length - 1));
                                id.update_state(ListUpdate::ScrollToSelected);
                            }
                        }
                    }
                    EventPropagation::Stop
                }
                Key::Named(NamedKey::ArrowDown) => {
                    let current = selection.get_untracked();
                    match current {
                        Some(i) => {
                            if i < length - 1 {
                                selection.set(Some(i + 1));
                                id.update_state(ListUpdate::ScrollToSelected);
                            }
                        }
                        None => {
                            if length > 0 {
                                selection.set(Some(0));
                                id.update_state(ListUpdate::ScrollToSelected);
                            }
                        }
                    }
                    EventPropagation::Stop
                }
                _ => EventPropagation::Continue,
            }
        } else {
            EventPropagation::Continue
        }
    })
}

impl View for List {
    fn view_data(&self) -> &ViewData {
        &self.data
    }

    fn view_data_mut(&mut self) -> &mut ViewData {
        &mut self.data
    }

    fn for_each_child<'a>(&'a self, for_each: &mut dyn FnMut(&'a dyn View) -> bool) {
        for_each(&self.child);
    }

    fn for_each_child_mut<'a>(&'a mut self, for_each: &mut dyn FnMut(&'a mut dyn View) -> bool) {
        for_each(&mut self.child);
    }

    fn for_each_child_rev_mut<'a>(
        &'a mut self,
        for_each: &mut dyn FnMut(&'a mut dyn View) -> bool,
    ) {
        for_each(&mut self.child);
    }

    fn debug_name(&self) -> std::borrow::Cow<'static, str> {
        "List".into()
    }

    fn update(&mut self, cx: &mut crate::context::UpdateCx, state: Box<dyn std::any::Any>) {
        if let Ok(change) = state.downcast::<ListUpdate>() {
            match *change {
                ListUpdate::SelectionChanged => {
                    cx.app_state_mut().request_style_recursive(self.id())
                }
                ListUpdate::ScrollToSelected => {
                    if let Some(index) = self.selection.get_untracked() {
                        self.child.children[index].id().scroll_to(None);
                    }
                }
            }
        }
    }
}

impl View for Item {
    fn view_data(&self) -> &ViewData {
        &self.data
    }

    fn view_data_mut(&mut self) -> &mut ViewData {
        &mut self.data
    }

    fn view_style(&self) -> Option<crate::style::Style> {
        Some(Style::new().flex_col())
    }

    fn for_each_child<'a>(&'a self, for_each: &mut dyn FnMut(&'a dyn View) -> bool) {
        for_each(&self.child);
    }

    fn for_each_child_mut<'a>(&'a mut self, for_each: &mut dyn FnMut(&'a mut dyn View) -> bool) {
        for_each(&mut self.child);
    }

    fn for_each_child_rev_mut<'a>(
        &'a mut self,
        for_each: &mut dyn FnMut(&'a mut dyn View) -> bool,
    ) {
        for_each(&mut self.child);
    }

    fn debug_name(&self) -> std::borrow::Cow<'static, str> {
        "Item".into()
    }

    fn style(&mut self, cx: &mut StyleCx<'_>) {
        let selected = self.selection.get_untracked();
        if Some(self.index) == selected {
            cx.save();
            cx.selected();
            cx.style_view(&mut self.child);
            cx.restore();
        } else {
            cx.style_view(&mut self.child);
        }
    }
}
