//! A browser for KernelOS.
//!
//! Real pages load in a sandboxed iframe. Plenty of the web refuses to be
//! framed (`X-Frame-Options` / CSP `frame-ancestors`) and there is no
//! client-side way around that, nor any way to reliably *detect* it — a blocked
//! frame fires `load` just like a successful cross-origin one. So rather than
//! guess and show a wrong error, every page keeps an "Open in new tab" escape
//! hatch, and the start page curates destinations known to frame cleanly.

use yew::prelude::*;
use web_sys::{HtmlInputElement, KeyboardEvent};
use std::rc::Rc;
use std::cell::RefCell;

use crate::filesystem::FileSystem;

/// Destinations that are known to allow framing, so the start page is never a
/// list of dead ends.
const BOOKMARKS: &[(&str, &str, &str)] = &[
    ("📚", "Wikipedia", "https://en.wikipedia.org/wiki/WebAssembly"),
    ("🦀", "Rust", "https://www.rust-lang.org"),
    ("📖", "The Rust Book", "https://doc.rust-lang.org/book/"),
    ("📦", "docs.rs", "https://docs.rs"),
    ("🗞️", "Hacker News", "https://news.ycombinator.com"),
    ("🧪", "example.com", "https://example.com"),
    ("📁", "Your files", "vfs:///home/documents"),
];

#[derive(Clone, PartialEq, Debug)]
pub enum Page {
    /// The built-in start page.
    Start,
    /// A file out of the KernelOS virtual filesystem.
    Vfs { path: String, content: Result<String, String> },
    /// A real web page in an iframe.
    Web { url: String },
}

impl Page {
    /// What belongs in the address bar for this page.
    fn address(&self) -> String {
        match self {
            Page::Start => String::new(),
            Page::Vfs { path, .. } => format!("vfs://{}", path),
            Page::Web { url } => url.clone(),
        }
    }
}

#[derive(Properties, Clone, PartialEq)]
pub struct BrowserProps {
    pub fs: Rc<RefCell<FileSystem>>,
    #[prop_or_default]
    pub initial_url: Option<String>,
}

pub struct Browser {
    /// Visited addresses, oldest first. Back/forward move `position` within it.
    history: Vec<String>,
    position: usize,
    page: Page,
    address_input: String,
    input_ref: NodeRef,
}

pub enum BrowserMsg {
    AddressChanged(String),
    AddressKeyDown(KeyboardEvent),
    Navigate(String),
    Back,
    Forward,
    Reload,
    Home,
    OpenExternally,
}

impl Component for Browser {
    type Message = BrowserMsg;
    type Properties = BrowserProps;

    fn create(ctx: &Context<Self>) -> Self {
        let mut browser = Self {
            history: Vec::new(),
            position: 0,
            page: Page::Start,
            address_input: String::new(),
            input_ref: NodeRef::default(),
        };

        if let Some(url) = ctx.props().initial_url.clone() {
            browser.go(&url, ctx);
        }

        browser
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            BrowserMsg::AddressChanged(value) => {
                self.address_input = value;
                true
            }
            BrowserMsg::AddressKeyDown(event) => {
                if event.key() == "Enter" {
                    let target = self.address_input.clone();
                    self.go(&target, ctx);
                }
                true
            }
            BrowserMsg::Navigate(url) => {
                self.go(&url, ctx);
                true
            }
            BrowserMsg::Back => {
                if self.position > 0 {
                    self.position -= 1;
                    self.load_current(ctx);
                }
                true
            }
            BrowserMsg::Forward => {
                if self.position + 1 < self.history.len() {
                    self.position += 1;
                    self.load_current(ctx);
                }
                true
            }
            BrowserMsg::Reload => {
                self.load_current(ctx);
                true
            }
            BrowserMsg::Home => {
                self.page = Page::Start;
                self.address_input.clear();
                true
            }
            BrowserMsg::OpenExternally => {
                if let Page::Web { url } = &self.page {
                    if let Some(window) = web_sys::window() {
                        let _ = window.open_with_url_and_target(url, "_blank");
                    }
                }
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        let can_go_back = self.position > 0 && !self.history.is_empty();
        let can_go_forward = self.position + 1 < self.history.len();

        html! {
            <div class="browser">
                <div class="browser-toolbar">
                    <button
                        class="browser-button"
                        disabled={!can_go_back}
                        onclick={link.callback(|_| BrowserMsg::Back)}
                        title="Back"
                    >{ "←" }</button>
                    <button
                        class="browser-button"
                        disabled={!can_go_forward}
                        onclick={link.callback(|_| BrowserMsg::Forward)}
                        title="Forward"
                    >{ "→" }</button>
                    <button
                        class="browser-button"
                        onclick={link.callback(|_| BrowserMsg::Reload)}
                        title="Reload"
                    >{ "⟳" }</button>
                    <button
                        class="browser-button"
                        onclick={link.callback(|_| BrowserMsg::Home)}
                        title="Start page"
                    >{ "⌂" }</button>

                    <input
                        ref={self.input_ref.clone()}
                        class="browser-address"
                        type="text"
                        placeholder="Enter a URL, vfs:///path, or search Wikipedia"
                        value={self.address_input.clone()}
                        oninput={link.callback(|e: InputEvent| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            BrowserMsg::AddressChanged(input.value())
                        })}
                        onkeydown={link.callback(BrowserMsg::AddressKeyDown)}
                    />

                    {
                        if matches!(self.page, Page::Web { .. }) {
                            html! {
                                <button
                                    class="browser-button browser-external"
                                    onclick={link.callback(|_| BrowserMsg::OpenExternally)}
                                    title="Open in a real browser tab"
                                >{ "↗" }</button>
                            }
                        } else {
                            html! {}
                        }
                    }
                </div>

                <div class="browser-viewport">
                    { self.render_page(ctx) }
                </div>
            </div>
        }
    }
}

impl Browser {
    /// Navigate to `input`, pushing a new history entry.
    fn go(&mut self, input: &str, ctx: &Context<Self>) {
        let target = Self::interpret(input);
        if target.is_empty() {
            self.page = Page::Start;
            self.address_input.clear();
            return;
        }

        // A new navigation truncates anything ahead of the current position,
        // the same way a real browser discards the forward stack.
        if !self.history.is_empty() {
            self.history.truncate(self.position + 1);
        }
        self.history.push(target);
        self.position = self.history.len() - 1;
        self.load_current(ctx);
    }

    fn load_current(&mut self, ctx: &Context<Self>) {
        let Some(target) = self.history.get(self.position).cloned() else {
            self.page = Page::Start;
            return;
        };

        self.page = if let Some(path) = target.strip_prefix("vfs://") {
            let path = FileSystem::normalize_path(path);
            let content = self.read_vfs(&path, ctx);
            Page::Vfs { path, content }
        } else {
            Page::Web { url: target }
        };

        self.address_input = self.page.address();
    }

    /// Render a virtual-filesystem path: files show their contents, directories
    /// show a listing.
    fn read_vfs(&self, path: &str, ctx: &Context<Self>) -> Result<String, String> {
        let fs = ctx.props().fs.borrow();

        if fs.is_directory(path) {
            let entries = fs.list_directory(path)?;
            if entries.is_empty() {
                return Ok("(empty directory)".to_string());
            }
            return Ok(entries
                .iter()
                .map(|entry| {
                    let marker = if matches!(entry.file_type, crate::filesystem::FileType::Directory) {
                        "/"
                    } else {
                        ""
                    };
                    format!("{}{}", entry.name, marker)
                })
                .collect::<Vec<_>>()
                .join("\n"));
        }

        fs.read_file(path)
    }

    /// Turn whatever the user typed into something navigable: a vfs path, a
    /// URL, or a Wikipedia search (the major search engines all refuse framing,
    /// so sending searches there would only ever produce a blank pane).
    fn interpret(input: &str) -> String {
        let input = input.trim();

        if input.is_empty() || input == "about:start" {
            return String::new();
        }

        if input.starts_with("vfs://") || input.starts_with("http://") || input.starts_with("https://") {
            return input.to_string();
        }

        if input.starts_with('/') {
            return format!("vfs://{}", input);
        }

        // A bare token with a dot and no spaces reads as a hostname.
        let looks_like_host = !input.contains(' ')
            && input.contains('.')
            && !input.ends_with('.');

        if looks_like_host {
            format!("https://{}", input)
        } else {
            format!(
                "https://en.wikipedia.org/w/index.php?search={}",
                js_sys::encode_uri_component(input)
            )
        }
    }

    /// Same-origin pages must not get `allow-same-origin`, or the framed
    /// document could reach back into KernelOS and out of the sandbox.
    fn sandbox_for(url: &str) -> &'static str {
        let same_origin = web_sys::window()
            .and_then(|w| w.location().origin().ok())
            .map(|origin| url.starts_with(&origin))
            .unwrap_or(false);

        if same_origin {
            "allow-scripts allow-forms"
        } else {
            "allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox"
        }
    }

    fn render_page(&self, ctx: &Context<Self>) -> Html {
        match &self.page {
            Page::Start => self.render_start_page(ctx),
            Page::Vfs { path, content } => html! {
                <div class="browser-vfs">
                    <div class="browser-vfs-path">{ path }</div>
                    {
                        match content {
                            Ok(text) => html! { <pre class="browser-vfs-content">{ text }</pre> },
                            Err(error) => html! {
                                <div class="browser-error">
                                    <div class="browser-error-icon">{ "🗂️" }</div>
                                    <p>{ error }</p>
                                </div>
                            },
                        }
                    }
                </div>
            },
            Page::Web { url } => html! {
                <>
                    <iframe
                        class="browser-frame"
                        src={url.clone()}
                        sandbox={Self::sandbox_for(url)}
                        referrerpolicy="no-referrer"
                    />
                    <div class="browser-hint">
                        { "Blank page? Some sites refuse to be embedded. Use ↗ to open it in a real tab." }
                    </div>
                </>
            },
        }
    }

    fn render_start_page(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();

        html! {
            <div class="browser-start">
                <div class="browser-start-logo">{ "🌐" }</div>
                <h1 class="browser-start-title">{ "KernelOS Browser" }</h1>
                <p class="browser-start-subtitle">
                    { "Type a URL, a " }
                    <code>{ "vfs:///path" }</code>
                    { " to browse your files, or anything else to search Wikipedia." }
                </p>
                <div class="browser-bookmarks">
                    {
                        BOOKMARKS.iter().map(|(icon, name, url)| {
                            let url = url.to_string();
                            html! {
                                <button
                                    class="browser-bookmark"
                                    onclick={link.callback(move |_| BrowserMsg::Navigate(url.clone()))}
                                >
                                    <span class="browser-bookmark-icon">{ icon }</span>
                                    <span class="browser-bookmark-name">{ name }</span>
                                </button>
                            }
                        }).collect::<Html>()
                    }
                </div>
            </div>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpret_passes_through_explicit_schemes() {
        assert_eq!(Browser::interpret("https://example.com"), "https://example.com");
        assert_eq!(Browser::interpret("http://example.com"), "http://example.com");
        assert_eq!(Browser::interpret("vfs:///home"), "vfs:///home");
    }

    #[test]
    fn interpret_treats_absolute_paths_as_vfs() {
        assert_eq!(Browser::interpret("/home/documents"), "vfs:///home/documents");
    }

    #[test]
    fn interpret_promotes_bare_hosts_to_https() {
        assert_eq!(Browser::interpret("example.com"), "https://example.com");
        assert_eq!(Browser::interpret("  rust-lang.org  "), "https://rust-lang.org");
    }

    #[test]
    fn interpret_blanks_out_the_start_page() {
        assert_eq!(Browser::interpret(""), "");
        assert_eq!(Browser::interpret("   "), "");
        assert_eq!(Browser::interpret("about:start"), "");
    }

    #[test]
    fn page_address_round_trips() {
        assert_eq!(Page::Start.address(), "");
        assert_eq!(
            Page::Web { url: "https://example.com".into() }.address(),
            "https://example.com"
        );
        assert_eq!(
            Page::Vfs { path: "/home".into(), content: Ok(String::new()) }.address(),
            "vfs:///home"
        );
    }
}
