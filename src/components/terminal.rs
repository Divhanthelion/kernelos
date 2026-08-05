use yew::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use web_sys::{HtmlInputElement, KeyboardEvent};
use crate::filesystem::{FileSystem, FileType};
use crate::plugin::abi::{describe_capability, Grant, PluginManifest};
use std::path::Path;

/// Stashed after `pkg install` fetches a manifest; the next line is y/N.
struct PendingInstall {
    manifest: PluginManifest,
    wasm_url: String,
    /// Original install spec, for status messages.
    spec: String,
}

pub struct Terminal {
    fs: Rc<RefCell<FileSystem>>,
    current_directory: String,
    /// Where `cd -` returns to.
    previous_directory: String,
    command_history: Vec<String>,
    history_index: Option<usize>,
    output_lines: Vec<TerminalLine>,
    current_input: String,
    input_ref: NodeRef,
    username: String,
    hostname: String,
    pending_install: Option<PendingInstall>,
}

#[derive(Clone, PartialEq)]
enum TerminalLine {
    Command { prompt: String, command: String },
    Output(String),
    Error(String),
}

pub enum TerminalMsg {
    InputChanged(String),
    ExecuteCommand,
    KeyDown(KeyboardEvent),
    Clear,
    /// Async command result (e.g. `pkg install`) appended to the output.
    Output(String),
    Error(String),
    /// Manifest fetched; show capability list and wait for y/N.
    InstallPrompt {
        manifest: PluginManifest,
        wasm_url: String,
        spec: String,
    },
}

#[derive(Properties, Clone, PartialEq)]
pub struct TerminalProps {
    pub fs: Rc<RefCell<FileSystem>>,
    #[prop_or_default]
    pub on_notification: Callback<(String, String, String)>,
}

impl Component for Terminal {
    type Message = TerminalMsg;
    type Properties = TerminalProps;

    fn create(ctx: &Context<Self>) -> Self {
        let mut terminal = Self {
            fs: Rc::clone(&ctx.props().fs),
            current_directory: "/home".to_string(),
            previous_directory: "/home".to_string(),
            command_history: Vec::new(),
            history_index: None,
            output_lines: Vec::new(),
            current_input: String::new(),
            input_ref: NodeRef::default(),
            username: "user".to_string(),
            hostname: "kernelosv2".to_string(),
            pending_install: None,
        };
        
        terminal.output_lines.push(TerminalLine::Output(
            "KernelOS Terminal v2.0".to_string()
        ));
        terminal.output_lines.push(TerminalLine::Output(
            "Type 'help' for available commands.".to_string()
        ));
        terminal.output_lines.push(TerminalLine::Output(String::new()));
        
        terminal
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            TerminalMsg::InputChanged(value) => {
                self.current_input = value;
                true
            }
            TerminalMsg::ExecuteCommand => {
                let command = self.current_input.trim().to_string();
                if self.pending_install.is_some() {
                    // [y/N]: bare Enter means decline.
                    self.handle_install_confirm(&command, ctx);
                    self.current_input.clear();
                    self.history_index = None;
                } else if !command.is_empty() {
                    self.execute_command(&command, ctx);
                    self.current_input.clear();
                    self.history_index = None;
                }
                true
            }
            TerminalMsg::KeyDown(event) => {
                match event.key().as_str() {
                    "Enter" => {
                        ctx.link().send_message(TerminalMsg::ExecuteCommand);
                        false
                    }
                    "ArrowUp" => {
                        event.prevent_default();
                        if !self.command_history.is_empty() {
                            let new_index = match self.history_index {
                                None => self.command_history.len() - 1,
                                Some(i) if i > 0 => i - 1,
                                Some(i) => i,
                            };
                            self.history_index = Some(new_index);
                            self.current_input = self.command_history[new_index].clone();
                        }
                        true
                    }
                    "ArrowDown" => {
                        event.prevent_default();
                        match self.history_index {
                            Some(i) if i < self.command_history.len() - 1 => {
                                self.history_index = Some(i + 1);
                                self.current_input = self.command_history[i + 1].clone();
                            }
                            Some(_) => {
                                self.history_index = None;
                                self.current_input.clear();
                            }
                            None => {}
                        }
                        true
                    }
                    "Tab" => {
                        event.prevent_default();
                        self.handle_tab_completion();
                        true
                    }
                    "l" if event.ctrl_key() => {
                        event.prevent_default();
                        self.output_lines.clear();
                        true
                    }
                    _ => false,
                }
            }
            TerminalMsg::Clear => {
                self.output_lines.clear();
                true
            }
            TerminalMsg::Output(text) => {
                self.output_lines.push(TerminalLine::Output(text));
                true
            }
            TerminalMsg::Error(text) => {
                self.output_lines.push(TerminalLine::Error(text));
                true
            }
            TerminalMsg::InstallPrompt {
                manifest,
                wasm_url,
                spec,
            } => {
                self.output_lines.push(TerminalLine::Output(format!(
                    "{} ({}) requests:",
                    manifest.name, manifest.id
                )));
                if manifest.requests.is_empty() {
                    self.output_lines
                        .push(TerminalLine::Output("  (none)".to_string()));
                } else {
                    for cap in &manifest.requests {
                        self.output_lines.push(TerminalLine::Output(format!(
                            "  - {}",
                            describe_capability(cap)
                        )));
                    }
                }
                self.output_lines
                    .push(TerminalLine::Output("Install? [y/N]".to_string()));
                self.pending_install = Some(PendingInstall {
                    manifest,
                    wasm_url,
                    spec,
                });
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let oninput = ctx.link().callback(|e: InputEvent| {
            let input: HtmlInputElement = e.target_unchecked_into();
            TerminalMsg::InputChanged(input.value())
        });
        
        let onkeydown = ctx.link().callback(TerminalMsg::KeyDown);
        
        let prompt = self.get_prompt();

        html! {
            <div class="terminal">
                <div
                    class="terminal-output"
                    onclick={ctx.link().callback(|_| {
                        // Focus input when clicking terminal
                        TerminalMsg::InputChanged(String::new())
                    })}
                >
                    {
                        self.output_lines.iter().map(|line| {
                            match line {
                                TerminalLine::Command { prompt, command } => {
                                    html! {
                                        <div class="terminal-line-command">
                                            <span class="terminal-prompt">{ prompt }</span>
                                            <span class="terminal-command">{ command }</span>
                                        </div>
                                    }
                                }
                                TerminalLine::Output(text) => {
                                    html! {
                                        <div class="terminal-line-output">{ text }</div>
                                    }
                                }
                                TerminalLine::Error(text) => {
                                    html! {
                                        <div class="terminal-line-error">{ text }</div>
                                    }
                                }
                            }
                        }).collect::<Html>()
                    }
                </div>
                <div class="terminal-input-line">
                    <span class="terminal-prompt">{ &prompt }</span>
                    <input 
                        type="text"
                        class="terminal-input"
                        value={self.current_input.clone()}
                        ref={self.input_ref.clone()}
                        {oninput}
                        {onkeydown}
                        autocomplete="off"
                        spellcheck="false"
                    />
                </div>
            </div>
        }
    }

    fn rendered(&mut self, _ctx: &Context<Self>, first_render: bool) {
        if first_render {
            if let Some(input) = self.input_ref.cast::<HtmlInputElement>() {
                let _ = input.focus();
            }
        }
        
        // Scroll to bottom
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            if let Ok(Some(el)) = doc.query_selector(".terminal-output") {
                el.set_scroll_top(el.scroll_height());
            }
        }
    }
}

impl Terminal {
    fn get_prompt(&self) -> String {
        let dir = if self.current_directory == "/home" {
            "~".to_string()
        } else if self.current_directory.starts_with("/home/") {
            format!("~{}", &self.current_directory[5..])
        } else {
            self.current_directory.clone()
        };
        
        format!("{}@{}:{} $ ", self.username, self.hostname, dir)
    }

    fn execute_command(&mut self, command: &str, ctx: &Context<Self>) {
        let prompt = self.get_prompt();
        self.output_lines.push(TerminalLine::Command {
            prompt,
            command: command.to_string(),
        });
        
        // Add to history
        if self.command_history.last().map(|s| s.as_str()) != Some(command) {
            self.command_history.push(command.to_string());
            if self.command_history.len() > 100 {
                self.command_history.remove(0);
            }
        }
        
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return;
        }

        match parts[0] {
            "help" => self.cmd_help(),
            "cd" => self.cmd_cd(&parts),
            "pwd" => self.cmd_pwd(),
            "ls" => self.cmd_ls(&parts),
            "cat" => self.cmd_cat(&parts),
            "echo" => self.cmd_echo(&parts),
            "clear" | "cls" => self.output_lines.clear(),
            "mkdir" => self.cmd_mkdir(&parts),
            "touch" => self.cmd_touch(&parts),
            "rm" => self.cmd_rm(&parts),
            "mv" => self.cmd_mv(&parts),
            "cp" => self.cmd_cp(&parts),
            "whoami" => self.output_lines.push(TerminalLine::Output(self.username.clone())),
            "hostname" => self.output_lines.push(TerminalLine::Output(self.hostname.clone())),
            "date" => self.cmd_date(),
            "history" => self.cmd_history(),
            "uname" => self.cmd_uname(&parts),
            "tree" => self.cmd_tree(&parts),
            "head" => self.cmd_head(&parts),
            "tail" => self.cmd_tail(&parts),
            "wc" => self.cmd_wc(&parts),
            "grep" => self.cmd_grep(&parts),
            "find" => self.cmd_find(&parts),
            "env" => self.cmd_env(),
            "pkg" => self.cmd_pkg(&parts, ctx),
            "exit" | "quit" => {
                self.output_lines.push(TerminalLine::Output(
                    "Use the window close button to exit.".to_string()
                ));
            }
            _ => {
                self.output_lines.push(TerminalLine::Error(
                    format!("Command not found: {}. Type 'help' for available commands.", parts[0])
                ));
            }
        }
    }

    /// Plugin package manager: `pkg install <id|url>`, `pkg list`, `pkg remove <id>`.
    /// Install is two-phase: fetch manifest → prompt for capability consent →
    /// on `y` complete the install with an explicit Grant.
    fn cmd_pkg(&mut self, parts: &[&str], ctx: &Context<Self>) {
        match parts.get(1).copied().unwrap_or("") {
            "install" => {
                let Some(spec) = parts.get(2) else {
                    self.output_lines.push(TerminalLine::Error(
                        "usage: pkg install <id|url>".to_string(),
                    ));
                    return;
                };
                if self.pending_install.is_some() {
                    self.output_lines.push(TerminalLine::Error(
                        "finish or cancel the pending install first (y/N)".to_string(),
                    ));
                    return;
                }
                let spec = spec.to_string();
                self.output_lines.push(TerminalLine::Output(format!(
                    "Fetching '{spec}'..."
                )));

                let link = ctx.link().clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let (manifest_url, wasm_url) = if spec.contains("://") {
                        let wasm_url = format!("{}.wasm", spec.trim_end_matches(".json"));
                        (spec.clone(), wasm_url)
                    } else {
                        (
                            format!("/plugins/{spec}.json"),
                            format!("/plugins/{spec}.wasm"),
                        )
                    };
                    match crate::plugin::fetch_plugin_manifest(&manifest_url).await {
                        Ok(manifest) => link.send_message(TerminalMsg::InstallPrompt {
                            manifest,
                            wasm_url,
                            spec,
                        }),
                        Err(e) => link.send_message(TerminalMsg::Error(format!(
                            "pkg install: {e}"
                        ))),
                    }
                });
            }
            "list" => {
                let installed = crate::plugin::apps();
                if installed.is_empty() {
                    self.output_lines
                        .push(TerminalLine::Output("No plugins installed.".to_string()));
                } else {
                    for app in installed {
                        self.output_lines.push(TerminalLine::Output(format!(
                            "{} — {} ({})",
                            app.id, app.name, app.category
                        )));
                    }
                }
            }
            "remove" => {
                let Some(id) = parts.get(2) else {
                    self.output_lines.push(TerminalLine::Error(
                        "usage: pkg remove <id>".to_string(),
                    ));
                    return;
                };
                let id = id.to_string();
                let fs = Rc::clone(&self.fs);
                let link = ctx.link().clone();
                let on_notification = ctx.props().on_notification.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match crate::plugin::uninstall(&id, &fs).await {
                        Ok(()) => {
                            on_notification.emit((
                                "Plugins".to_string(),
                                format!("Removed '{id}'."),
                                "success".to_string(),
                            ));
                            link.send_message(TerminalMsg::Output(format!(
                                "Removed '{id}'."
                            )));
                        }
                        Err(e) => link.send_message(TerminalMsg::Error(format!(
                            "pkg remove: {e}"
                        ))),
                    }
                });
            }
            _ => {
                self.output_lines.push(TerminalLine::Output(
                    "Usage: pkg install <id|url> | pkg list | pkg remove <id>".to_string(),
                ));
            }
        }
    }

    /// Interpret the next line as the y/N answer for a pending `pkg install`.
    fn handle_install_confirm(&mut self, answer: &str, ctx: &Context<Self>) {
        let prompt = self.get_prompt();
        self.output_lines.push(TerminalLine::Command {
            prompt,
            command: answer.to_string(),
        });

        let pending = match self.pending_install.take() {
            Some(p) => p,
            None => return,
        };

        let confirmed = matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes");
        if !confirmed {
            self.output_lines.push(TerminalLine::Output(format!(
                "Cancelled install of '{}'.",
                pending.spec
            )));
            return;
        }

        self.output_lines.push(TerminalLine::Output(format!(
            "Installing '{}'...",
            pending.spec
        )));

        let fs = Rc::clone(&self.fs);
        let on_notify = {
            let cb = ctx.props().on_notification.clone();
            Callback::from(move |(title, body): (String, String)| {
                cb.emit((title, body, "info".to_string()));
            })
        };
        let link = ctx.link().clone();
        let on_installed = ctx.props().on_notification.clone();
        let grant = Grant(pending.manifest.requests.clone());
        let spec = pending.spec.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match crate::plugin::complete_install(
                &fs,
                pending.manifest,
                &pending.wasm_url,
                grant,
                on_notify,
            )
            .await
            {
                Ok(()) => {
                    on_installed.emit((
                        "Plugins".to_string(),
                        format!("Installed '{spec}'."),
                        "success".to_string(),
                    ));
                    link.send_message(TerminalMsg::Output(format!(
                        "Installed '{spec}'."
                    )));
                }
                Err(e) => link.send_message(TerminalMsg::Error(format!(
                    "pkg install: {e}"
                ))),
            }
        });
    }

    fn cmd_help(&mut self) {
        let help_text = r#"Available commands:

  Navigation:
    cd [path]     - Change directory
    pwd           - Print working directory
    ls [path]     - List directory contents
    tree [path]   - Display directory tree

  File Operations:
    cat [file]    - Display file contents
    head [file]   - Display first 10 lines
    tail [file]   - Display last 10 lines
    touch [file]  - Create empty file
    mkdir [dir]   - Create directory
    rm [-r] [path] - Remove file/directory
    mv [src] [dst] - Move/rename file
    cp [src] [dst] - Copy file

  Search:
    grep [pattern] [file] - Search in file
    find [path] [name]    - Find files
    wc [file]            - Word/line count

  System:
    whoami        - Display username
    hostname      - Display hostname
    date          - Display date/time
    uname [-a]    - System information
    env           - Environment variables
    history       - Command history
    clear         - Clear terminal

  Misc:
    echo [text]   - Display text
    help          - Show this help"#;
        
        self.output_lines.push(TerminalLine::Output(help_text.to_string()));
    }

    fn cmd_cd(&mut self, parts: &[&str]) {
        // `..` needs no special case now that resolve_path canonicalizes.
        let target = match parts.get(1) {
            None => "/home".to_string(),
            Some(&"-") => self.previous_directory.clone(),
            Some(path) => self.resolve_path(path),
        };

        let departing_from = self.current_directory.clone();
        
        match self.fs.borrow().list_directory(&target) {
            Ok(_) => {
                self.current_directory = target;
                self.previous_directory = departing_from;
            }
            Err(e) => self.output_lines.push(TerminalLine::Error(format!("cd: {}", e))),
        }
    }

    fn cmd_pwd(&mut self) {
        self.output_lines.push(TerminalLine::Output(self.current_directory.clone()));
    }

    fn cmd_ls(&mut self, parts: &[&str]) {
        let show_all = parts.contains(&"-a") || parts.contains(&"-la") || parts.contains(&"-al");
        let long_format = parts.contains(&"-l") || parts.contains(&"-la") || parts.contains(&"-al");
        
        let path = parts.iter()
            .skip(1)
            .find(|p| !p.starts_with('-'))
            .map(|p| self.resolve_path(p))
            .unwrap_or_else(|| self.current_directory.clone());
        
        match self.fs.borrow().list_directory(&path) {
            Ok(files) => {
                let files: Vec<_> = files
                    .into_iter()
                    .filter(|f| show_all || !f.name.starts_with('.'))
                    .collect();
                if files.is_empty() {
                    return;
                }
                
                if long_format {
                    for file in &files {
                        let type_char = match file.file_type {
                            FileType::Directory => 'd',
                            FileType::File => '-',
                        };
                        let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(file.modified as f64));
                        let date_str = format!("{}/{}/{}", 
                            date.get_month() + 1, 
                            date.get_date(),
                            date.get_full_year()
                        );
                        
                        let line = format!(
                            "{}rwxr-xr-x  1 user  {:>8}  {}  {}",
                            type_char,
                            if matches!(file.file_type, FileType::Directory) { "-".to_string() } else { file.size.to_string() },
                            date_str,
                            file.name
                        );
                        self.output_lines.push(TerminalLine::Output(line));
                    }
                } else {
                    let names: Vec<String> = files.iter()
                        .map(|f| {
                            if matches!(f.file_type, FileType::Directory) {
                                format!("{}/", f.name)
                            } else {
                                f.name.clone()
                            }
                        })
                        .collect();
                    self.output_lines.push(TerminalLine::Output(names.join("  ")));
                }
            }
            Err(e) => self.output_lines.push(TerminalLine::Error(format!("ls: {}", e))),
        }
    }

    fn cmd_cat(&mut self, parts: &[&str]) {
        if parts.len() < 2 {
            self.output_lines.push(TerminalLine::Error("cat: missing file operand".to_string()));
            return;
        }
        
        for file in parts.iter().skip(1) {
            let path = self.resolve_path(file);
            match self.fs.borrow().read_file(&path) {
                Ok(content) => {
                    for line in content.lines() {
                        self.output_lines.push(TerminalLine::Output(line.to_string()));
                    }
                }
                Err(e) => self.output_lines.push(TerminalLine::Error(format!("cat: {}: {}", file, e))),
            }
        }
    }

    fn cmd_echo(&mut self, parts: &[&str]) {
        let text = parts.iter().skip(1).copied().collect::<Vec<_>>().join(" ");
        self.output_lines.push(TerminalLine::Output(text));
    }

    fn cmd_mkdir(&mut self, parts: &[&str]) {
        if parts.len() < 2 {
            self.output_lines.push(TerminalLine::Error("mkdir: missing directory name".to_string()));
            return;
        }
        
        let create_parents = parts.contains(&"-p");
        
        for dir in parts.iter().skip(1).filter(|p| !p.starts_with('-')) {
            let path = self.resolve_path(dir);
            match self.fs.borrow_mut().create_directory(&path, create_parents) {
                Ok(_) => {}
                Err(e) => self.output_lines.push(TerminalLine::Error(format!("mkdir: {}: {}", dir, e))),
            }
        }
    }

    fn cmd_touch(&mut self, parts: &[&str]) {
        if parts.len() < 2 {
            self.output_lines.push(TerminalLine::Error("touch: missing file operand".to_string()));
            return;
        }
        
        for file in parts.iter().skip(1) {
            let path = self.resolve_path(file);
            if !self.fs.borrow().exists(&path) {
                match self.fs.borrow_mut().write_file(&path, "") {
                    Ok(_) => {}
                    Err(e) => self.output_lines.push(TerminalLine::Error(format!("touch: {}: {}", file, e))),
                }
            }
        }
    }

    fn cmd_rm(&mut self, parts: &[&str]) {
        if parts.len() < 2 {
            self.output_lines.push(TerminalLine::Error("rm: missing operand".to_string()));
            return;
        }
        
        let recursive = parts.contains(&"-r") || parts.contains(&"-rf") || parts.contains(&"-fr");
        
        for path in parts.iter().skip(1).filter(|p| !p.starts_with('-')) {
            let full_path = self.resolve_path(path);
            match self.fs.borrow_mut().delete(&full_path, recursive) {
                Ok(_) => {}
                Err(e) => self.output_lines.push(TerminalLine::Error(format!("rm: {}: {}", path, e))),
            }
        }
    }

    fn cmd_mv(&mut self, parts: &[&str]) {
        if parts.len() < 3 {
            self.output_lines.push(TerminalLine::Error("mv: missing operand".to_string()));
            return;
        }
        
        let src = self.resolve_path(parts[1]);
        let dst = self.resolve_path(parts[2]);
        
        match self.fs.borrow_mut().rename(&src, &dst) {
            Ok(_) => {}
            Err(e) => self.output_lines.push(TerminalLine::Error(format!("mv: {}", e))),
        }
    }

    fn cmd_cp(&mut self, parts: &[&str]) {
        if parts.len() < 3 {
            self.output_lines.push(TerminalLine::Error("cp: missing operand".to_string()));
            return;
        }
        
        let src = self.resolve_path(parts[1]);
        let dst = self.resolve_path(parts[2]);
        
        match self.fs.borrow_mut().copy(&src, &dst) {
            Ok(_) => {}
            Err(e) => self.output_lines.push(TerminalLine::Error(format!("cp: {}", e))),
        }
    }

    fn cmd_date(&mut self) {
        let date = js_sys::Date::new_0();
        let days = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
        let months = ["January", "February", "March", "April", "May", "June", 
                      "July", "August", "September", "October", "November", "December"];
        
        let output = format!(
            "{}, {} {}, {} {:02}:{:02}:{:02}",
            days[date.get_day() as usize],
            months[date.get_month() as usize],
            date.get_date(),
            date.get_full_year(),
            date.get_hours(),
            date.get_minutes(),
            date.get_seconds()
        );
        
        self.output_lines.push(TerminalLine::Output(output));
    }

    fn cmd_history(&mut self) {
        if self.command_history.is_empty() {
            self.output_lines.push(TerminalLine::Output("No command history".to_string()));
        } else {
            for (i, cmd) in self.command_history.iter().enumerate() {
                self.output_lines.push(TerminalLine::Output(format!("{:4}  {}", i + 1, cmd)));
            }
        }
    }

    fn cmd_uname(&mut self, parts: &[&str]) {
        if parts.contains(&"-a") {
            self.output_lines.push(TerminalLine::Output(
                "KernelOS 2.0.0 wasm32 WebAssembly Browser".to_string()
            ));
        } else {
            self.output_lines.push(TerminalLine::Output("KernelOS".to_string()));
        }
    }

    fn cmd_tree(&mut self, parts: &[&str]) {
        let path = parts.get(1)
            .map(|p| self.resolve_path(p))
            .unwrap_or_else(|| self.current_directory.clone());
        
        self.output_lines.push(TerminalLine::Output(path.clone()));
        self.print_tree(&path, "", true);
    }

    fn print_tree(&mut self, path: &str, prefix: &str, _is_last: bool) {
        let files = match self.fs.borrow().list_directory(path) {
            Ok(f) => f,
            Err(_) => return,
        };
        let count = files.len();
        for (i, file) in files.iter().enumerate() {
            let is_last_item = i == count - 1;
            let connector = if is_last_item { "└── " } else { "├── " };
            let name = if matches!(file.file_type, FileType::Directory) {
                format!("{}/", file.name)
            } else {
                file.name.clone()
            };

            self.output_lines.push(TerminalLine::Output(format!("{}{}{}", prefix, connector, name)));

            if matches!(file.file_type, FileType::Directory) {
                let new_prefix = format!("{}{}   ", prefix, if is_last_item { " " } else { "│" });
                let child_path = format!("{}/{}", path, file.name);
                self.print_tree(&child_path, &new_prefix, is_last_item);
            }
        }
    }

    fn cmd_head(&mut self, parts: &[&str]) {
        let lines_count = 10;
        
        if parts.len() < 2 {
            self.output_lines.push(TerminalLine::Error("head: missing file operand".to_string()));
            return;
        }
        
        let path = self.resolve_path(parts[1]);
        match self.fs.borrow().read_file(&path) {
            Ok(content) => {
                for line in content.lines().take(lines_count) {
                    self.output_lines.push(TerminalLine::Output(line.to_string()));
                }
            }
            Err(e) => self.output_lines.push(TerminalLine::Error(format!("head: {}", e))),
        }
    }

    fn cmd_tail(&mut self, parts: &[&str]) {
        let lines_count = 10;
        
        if parts.len() < 2 {
            self.output_lines.push(TerminalLine::Error("tail: missing file operand".to_string()));
            return;
        }
        
        let path = self.resolve_path(parts[1]);
        match self.fs.borrow().read_file(&path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let start = lines.len().saturating_sub(lines_count);
                for line in lines.into_iter().skip(start) {
                    self.output_lines.push(TerminalLine::Output(line.to_string()));
                }
            }
            Err(e) => self.output_lines.push(TerminalLine::Error(format!("tail: {}", e))),
        }
    }

    fn cmd_wc(&mut self, parts: &[&str]) {
        if parts.len() < 2 {
            self.output_lines.push(TerminalLine::Error("wc: missing file operand".to_string()));
            return;
        }
        
        let path = self.resolve_path(parts[1]);
        match self.fs.borrow().read_file(&path) {
            Ok(content) => {
                let lines = content.lines().count();
                let words = content.split_whitespace().count();
                let chars = content.len();
                self.output_lines.push(TerminalLine::Output(
                    format!("{:>8} {:>8} {:>8} {}", lines, words, chars, parts[1])
                ));
            }
            Err(e) => self.output_lines.push(TerminalLine::Error(format!("wc: {}", e))),
        }
    }

    fn cmd_grep(&mut self, parts: &[&str]) {
        if parts.len() < 3 {
            self.output_lines.push(TerminalLine::Error("grep: usage: grep <pattern> <file>".to_string()));
            return;
        }
        
        let pattern = parts[1].to_lowercase();
        let path = self.resolve_path(parts[2]);
        
        match self.fs.borrow().read_file(&path) {
            Ok(content) => {
                for (i, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(&pattern) {
                        self.output_lines.push(TerminalLine::Output(
                            format!("{}:{}", i + 1, line)
                        ));
                    }
                }
            }
            Err(e) => self.output_lines.push(TerminalLine::Error(format!("grep: {}", e))),
        }
    }

    fn cmd_find(&mut self, parts: &[&str]) {
        let (search_path, name_pattern) = match parts.len() {
            1 => (self.current_directory.clone(), None),
            2 => (self.resolve_path(parts[1]), None),
            _ => {
                if parts[2] == "-name" && parts.len() > 3 {
                    (self.resolve_path(parts[1]), Some(parts[3].to_lowercase()))
                } else {
                    (self.resolve_path(parts[1]), Some(parts[2].to_lowercase()))
                }
            }
        };
        
        self.find_recursive(&search_path, &name_pattern);
    }

    fn find_recursive(&mut self, path: &str, pattern: &Option<String>) {
        let files = match self.fs.borrow().list_directory(path) {
            Ok(f) => f,
            Err(_) => return,
        };
        for file in files {
            let full_path = format!("{}/{}", path, file.name);

            let matches = pattern.as_ref()
                .map(|p| file.name.to_lowercase().contains(p))
                .unwrap_or(true);

            if matches {
                self.output_lines.push(TerminalLine::Output(full_path.clone()));
            }

            if matches!(file.file_type, FileType::Directory) {
                self.find_recursive(&full_path, pattern);
            }
        }
    }

    fn cmd_env(&mut self) {
        let env_vars = [
            ("USER", &self.username),
            ("HOME", &"/home".to_string()),
            ("PWD", &self.current_directory),
            ("SHELL", &"/bin/bash".to_string()),
            ("TERM", &"xterm-256color".to_string()),
            ("HOSTNAME", &self.hostname),
        ];
        
        for (key, value) in &env_vars {
            self.output_lines.push(TerminalLine::Output(format!("{}={}", key, value)));
        }
    }

    fn handle_tab_completion(&mut self) {
        let input = self.current_input.clone();
        let parts: Vec<&str> = input.split_whitespace().collect();
        
        if parts.is_empty() {
            return;
        }
        
        // Command completion
        if parts.len() == 1 && !input.ends_with(' ') {
            let commands = vec![
                "cat", "cd", "clear", "cp", "date", "echo", "env", "find", 
                "grep", "head", "help", "history", "hostname", "ls", "mkdir", 
                "mv", "pwd", "rm", "tail", "touch", "tree", "uname", "wc", "whoami"
            ];
            
            let matches: Vec<&str> = commands.iter()
                .filter(|c| c.starts_with(parts[0]))
                .copied()
                .collect();
            
            if matches.len() == 1 {
                self.current_input = format!("{} ", matches[0]);
            } else if !matches.is_empty() {
                self.output_lines.push(TerminalLine::Output(matches.join("  ")));
            }
            return;
        }
        
        // Path completion
        let path_part = if input.ends_with(' ') { "" } else { parts.last().unwrap_or(&"") };
        let (dir_path, prefix) = if path_part.contains('/') {
            let p = Path::new(path_part);
            (
                self.resolve_path(&p.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()),
                p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()
            )
        } else {
            (self.current_directory.clone(), path_part.to_string())
        };
        
        if let Ok(files) = self.fs.borrow().list_directory(&dir_path) {
            let matches: Vec<String> = files.iter()
                .filter(|f| f.name.starts_with(&prefix))
                .map(|f| {
                    if matches!(f.file_type, FileType::Directory) {
                        format!("{}/", f.name)
                    } else {
                        f.name.clone()
                    }
                })
                .collect();
            
            if matches.len() == 1 {
                let completed = if path_part.contains('/') {
                    let p = Path::new(path_part);
                    format!("{}/{}", p.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(), matches[0])
                } else {
                    matches[0].clone()
                };
                
                let prefix_parts = &parts[..parts.len() - 1];
                self.current_input = format!("{} {}", prefix_parts.join(" "), completed);
            } else if !matches.is_empty() {
                self.output_lines.push(TerminalLine::Output(matches.join("  ")));
            }
        }
    }

    /// Expand `~`, anchor relative paths to the working directory, and collapse
    /// `.`/`..` so every command sees a canonical absolute path.
    fn resolve_path(&self, path: &str) -> String {
        let joined = if path.starts_with('/') {
            path.to_string()
        } else if path == "~" {
            "/home".to_string()
        } else if let Some(rest) = path.strip_prefix("~/") {
            format!("/home/{}", rest)
        } else {
            format!("{}/{}", self.current_directory, path)
        };

        FileSystem::normalize_path(&joined)
    }
}
