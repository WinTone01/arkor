//! Text fields bound to settings. A page asks for a field by key while rendering; the input
//! is created once (with the current value) and kept until the page changes, and every edit
//! goes straight to `on_change`, which updates and saves the setting.

use std::collections::HashMap;

use gpui::{AppContext, Context, Entity, Subscription};

use super::Library;
use super::text_input::{InputEvent, TextInput};

#[derive(Default)]
pub(super) struct Fields {
    inputs: HashMap<String, (Entity<TextInput>, Subscription)>,
}

impl Fields {
    pub(super) fn clear(&mut self) {
        self.inputs.clear();
    }

    pub(super) fn remove(&mut self, key: &str) {
        self.inputs.remove(key);
    }
}

impl Library {
    /// A field whose every change calls `on_change` (which should update and save state).
    pub(super) fn field(
        &mut self,
        key: impl Into<String>,
        initial: impl FnOnce(&Self) -> String,
        placeholder: &'static str,
        cx: &mut Context<Self>,
        on_change: impl Fn(&mut Self, &str) + 'static,
    ) -> Entity<TextInput> {
        let key = key.into();
        if let Some((input, _)) = self.fields.inputs.get(&key) {
            return input.clone();
        }
        let value = initial(self);
        let input = cx.new(|cx| TextInput::field(value, placeholder, cx));
        let subscription = cx.subscribe(&input, move |this, _, event: &InputEvent, cx| {
            if let InputEvent::Changed(text) = event {
                on_change(this, text);
                cx.notify();
            }
        });
        self.fields
            .inputs
            .insert(key, (input.clone(), subscription));
        input
    }

    /// A field that only acts on Enter (e.g. "add KEY=VALUE"); it is cleared afterwards.
    pub(super) fn submit_field(
        &mut self,
        key: impl Into<String>,
        placeholder: &'static str,
        cx: &mut Context<Self>,
        on_submit: impl Fn(&mut Self, &str, &mut Context<Self>) + 'static,
    ) -> Entity<TextInput> {
        let key = key.into();
        if let Some((input, _)) = self.fields.inputs.get(&key) {
            return input.clone();
        }
        let input = cx.new(|cx| TextInput::field("", placeholder, cx));
        let subscription = cx.subscribe(&input, move |this, input, event: &InputEvent, cx| {
            if let InputEvent::Submit(text) = event {
                on_submit(this, text, cx);
                input.update(cx, |i, cx| i.set_text("", cx));
                cx.notify();
            }
        });
        self.fields
            .inputs
            .insert(key, (input.clone(), subscription));
        input
    }

    /// Current text of a field created earlier with `field`/`submit_field`.
    pub(super) fn field_text(&self, key: &str, cx: &gpui::App) -> Option<String> {
        self.fields
            .inputs
            .get(key)
            .map(|(input, _)| input.read(cx).text())
    }
}
