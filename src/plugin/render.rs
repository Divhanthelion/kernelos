//! Interpret plugin UiOp descriptions into Yew Html.

use yew::prelude::*;

use crate::plugin::abi::{Event, UiOp};

pub struct RenderContext {
    pub on_event: Callback<Event>,
}

pub fn render_ops(ops: &[UiOp], ctx: &RenderContext) -> Html {
    let mut stack: Vec<Vec<Html>> = vec![vec![]];

    for op in ops {
        match op {
            UiOp::BeginVBox { gap } => {
                stack.push(vec![]);
                let _ = gap; // gap applied via CSS class for now
            }
            UiOp::BeginHBox { gap } => {
                stack.push(vec![]);
                let _ = gap;
            }
            UiOp::End => {
                if stack.len() > 1 {
                    let children = stack.pop().unwrap_or_default();
                    let inner = html! { <>{ for children }</> };
                    if let Some(parent) = stack.last_mut() {
                        parent.push(inner);
                    }
                }
            }
            UiOp::Label { text, class } => {
                // html! produces a 'static VNode, so break the borrow from
                // `ops` by cloning into the node.
                let class = class.as_deref().unwrap_or("plugin-label").to_string();
                let node = html! { <p class={class}>{ text.clone() }</p> };
                if let Some(parent) = stack.last_mut() {
                    parent.push(node);
                }
            }
            UiOp::Button { id, text } => {
                let on_event = ctx.on_event.clone();
                let btn_id = *id;
                let node = html! {
                    <button
                        class="plugin-button"
                        onclick={move |e: MouseEvent| {
                            e.stop_propagation();
                            on_event.emit(Event::Click { id: btn_id });
                        }}
                    >
                        { text }
                    </button>
                };
                if let Some(parent) = stack.last_mut() {
                    parent.push(node);
                }
            }
            UiOp::Input {
                id,
                value,
                placeholder,
            } => {
                let on_event = ctx.on_event.clone();
                let input_id = *id;
                let node = html! {
                    <input
                        class="plugin-input"
                        type="text"
                        value={value.clone()}
                        placeholder={placeholder.clone().unwrap_or_default()}
                        oninput={move |e: InputEvent| {
                            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                            on_event.emit(Event::InputChanged {
                                id: input_id,
                                value: input.value(),
                            });
                        }}
                    />
                };
                if let Some(parent) = stack.last_mut() {
                    parent.push(node);
                }
            }
            UiOp::Checkbox { id, checked, label } => {
                let on_event = ctx.on_event.clone();
                let cb_id = *id;
                let node = html! {
                    <label class="plugin-checkbox">
                        <input
                            type="checkbox"
                            checked={*checked}
                            onclick={move |e: MouseEvent| {
                                e.stop_propagation();
                                let input: web_sys::HtmlInputElement =
                                    e.target_unchecked_into();
                                on_event.emit(Event::Click { id: cb_id });
                                let _ = input;
                            }}
                        />
                        { label }
                    </label>
                };
                if let Some(parent) = stack.last_mut() {
                    parent.push(node);
                }
            }
            UiOp::List { items, selected } => {
                let node = html! {
                    <ul class="plugin-list">
                        {
                            for items.iter().enumerate().map(|(i, item)| {
                                let sel = selected.map(|s| s == i).unwrap_or(false);
                                html! {
                                    <li class={if sel { "selected" } else { "" }}>{ item }</li>
                                }
                            })
                        }
                    </ul>
                };
                if let Some(parent) = stack.last_mut() {
                    parent.push(node);
                }
            }
        }
    }

    let root = stack.pop().unwrap_or_default();
    html! {
        <div class="plugin-ui">
            { for root }
        </div>
    }
}
