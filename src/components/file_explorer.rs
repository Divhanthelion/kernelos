use yew::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::filesystem::{FileSystem, FileType, FileMetadata};
use wasm_bindgen::JsValue;

pub struct FileExplorer {
    fs: Rc<RefCell<FileSystem>>,
    current_path: String,
    files: Vec<FileMetadata>,
    selected_file: Option<String>,
    error_message: Option<String>,
    view_mode: ViewMode,
    sort_by: SortBy,
    sort_ascending: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    List,
    Grid,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SortBy {
    Name,
    Size,
    Modified,
}

pub enum FileExplorerMsg {
    NavigateTo(String),
    NavigateUp,
    Refresh,
    SelectFile(String),
    OpenFile(String),
    DeleteFile(String),
    CreateNewFile,
    CreateNewDirectory,
    ToggleViewMode,
    SortBy(SortBy),
    ClearError,
}

#[derive(Properties, Clone, PartialEq)]
pub struct FileExplorerProps {
    pub fs: Rc<RefCell<FileSystem>>,
    pub on_open_file: Callback<(String, String)>,
}

impl Component for FileExplorer {
    type Message = FileExplorerMsg;
    type Properties = FileExplorerProps;

    fn create(ctx: &Context<Self>) -> Self {
        let fs = Rc::clone(&ctx.props().fs);
        let current_path = "/home".to_string();
        
        let files = fs.borrow().list_directory(&current_path).unwrap_or_default();

        Self {
            fs,
            current_path,
            files,
            selected_file: None,
            error_message: None,
            view_mode: ViewMode::List,
            sort_by: SortBy::Name,
            sort_ascending: true,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            FileExplorerMsg::NavigateTo(path) => {
                let result = self.fs.borrow().list_directory(&path);
                match result {
                    Ok(files) => {
                        self.current_path = path;
                        self.files = files;
                        self.selected_file = None;
                        self.sort_files();
                    }
                    Err(e) => self.error_message = Some(e),
                }
                true
            }
            FileExplorerMsg::NavigateUp => {
                let parent = std::path::Path::new(&self.current_path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string());
                
                ctx.link().send_message(FileExplorerMsg::NavigateTo(parent));
                false
            }
            FileExplorerMsg::Refresh => {
                let result = self.fs.borrow().list_directory(&self.current_path);
                match result {
                    Ok(files) => {
                        self.files = files;
                        self.sort_files();
                    }
                    Err(e) => self.error_message = Some(e),
                }
                true
            }
            FileExplorerMsg::SelectFile(name) => {
                self.selected_file = if self.selected_file.as_ref() == Some(&name) {
                    None
                } else {
                    Some(name)
                };
                true
            }
            FileExplorerMsg::OpenFile(name) => {
                let full_path = format!("{}/{}", self.current_path, name);
                
                for file in &self.files {
                    if file.name == name {
                        match file.file_type {
                            FileType::Directory => {
                                ctx.link().send_message(FileExplorerMsg::NavigateTo(full_path));
                                return false;
                            }
                            FileType::File => {
                                ctx.props().on_open_file.emit((full_path, "text".to_string()));
                                return false;
                            }
                        }
                    }
                }
                false
            }
            FileExplorerMsg::DeleteFile(name) => {
                let full_path = format!("{}/{}", self.current_path, name);
                
                match self.fs.borrow_mut().delete(&full_path, true) {
                    Ok(_) => ctx.link().send_message(FileExplorerMsg::Refresh),
                    Err(e) => self.error_message = Some(e),
                }
                true
            }
            FileExplorerMsg::CreateNewFile => {
                let mut counter = 1;
                let mut new_name = "untitled.txt".to_string();
                
                while self.files.iter().any(|f| f.name == new_name) {
                    new_name = format!("untitled_{}.txt", counter);
                    counter += 1;
                }
                
                let new_path = format!("{}/{}", self.current_path, new_name);
                match self.fs.borrow_mut().write_file(&new_path, "") {
                    Ok(_) => ctx.link().send_message(FileExplorerMsg::Refresh),
                    Err(e) => self.error_message = Some(e),
                }
                true
            }
            FileExplorerMsg::CreateNewDirectory => {
                let mut counter = 1;
                let mut new_name = "New Folder".to_string();
                
                while self.files.iter().any(|f| f.name == new_name) {
                    new_name = format!("New Folder ({})", counter);
                    counter += 1;
                }
                
                let new_path = format!("{}/{}", self.current_path, new_name);
                match self.fs.borrow_mut().create_directory(&new_path, false) {
                    Ok(_) => ctx.link().send_message(FileExplorerMsg::Refresh),
                    Err(e) => self.error_message = Some(e),
                }
                true
            }
            FileExplorerMsg::ToggleViewMode => {
                self.view_mode = match self.view_mode {
                    ViewMode::List => ViewMode::Grid,
                    ViewMode::Grid => ViewMode::List,
                };
                true
            }
            FileExplorerMsg::SortBy(sort_by) => {
                if self.sort_by == sort_by {
                    self.sort_ascending = !self.sort_ascending;
                } else {
                    self.sort_by = sort_by;
                    self.sort_ascending = true;
                }
                self.sort_files();
                true
            }
            FileExplorerMsg::ClearError => {
                self.error_message = None;
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let path_parts: Vec<String> = self.current_path
            .split('/')
            .filter(|p| !p.is_empty())
            .map(|s| s.to_string())
            .collect();

        html! {
            <div class="file-explorer">
                // Toolbar
                <div class="file-explorer-toolbar">
                    <button 
                        class="btn btn-secondary"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::NavigateUp)}
                    >
                        { "↑ Up" }
                    </button>
                    <button 
                        class="btn btn-secondary"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::Refresh)}
                    >
                        { "⟳" }
                    </button>
                    <div class="file-explorer-spacer" />
                    <button 
                        class="btn btn-primary"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::CreateNewFile)}
                    >
                        { "+ File" }
                    </button>
                    <button 
                        class="btn btn-primary"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::CreateNewDirectory)}
                    >
                        { "+ Folder" }
                    </button>
                    <button 
                        class="btn btn-secondary"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::ToggleViewMode)}
                    >
                        { if self.view_mode == ViewMode::List { "☷" } else { "☰" } }
                    </button>
                </div>
                
                // Path bar
                <div class="file-explorer-path">
                    <button 
                        class="file-explorer-path-segment"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::NavigateTo("/".to_string()))}
                    >
                        { "/" }
                    </button>
                    {
                        path_parts.iter().enumerate().map(|(i, part)| {
                            let path = format!("/{}", path_parts[0..=i].join("/"));
                            html! {
                                <>
                                    <span class="file-explorer-path-separator">{ "›" }</span>
                                    <button 
                                        class="file-explorer-path-segment"
                                        onclick={ctx.link().callback(move |_| FileExplorerMsg::NavigateTo(path.clone()))}
                                    >
                                        { part }
                                    </button>
                                </>
                            }
                        }).collect::<Html>()
                    }
                </div>
                
                // Error message
                {
                    if let Some(error) = &self.error_message {
                        html! {
                            <div class="file-explorer-error">
                                <span>{ error }</span>
                                <button 
                                    class="file-explorer-error-dismiss"
                                    onclick={ctx.link().callback(|_| FileExplorerMsg::ClearError)}
                                >
                                    { "×" }
                                </button>
                            </div>
                        }
                    } else {
                        html! {}
                    }
                }
                
                // File list
                <div class="file-explorer-content">
                    {
                        if self.view_mode == ViewMode::List {
                            self.render_list_view(ctx)
                        } else {
                            self.render_grid_view(ctx)
                        }
                    }
                </div>
                
                // Status bar
                <div class="file-explorer-status">
                    { format!("{} items", self.files.len()) }
                    {
                        if let Some(selected) = &self.selected_file {
                            html! { <span>{ format!(" • {} selected", selected) }</span> }
                        } else {
                            html! {}
                        }
                    }
                </div>
            </div>
        }
    }
}

impl FileExplorer {
    fn sort_files(&mut self) {
        self.files.sort_by(|a, b| {
            // Always put directories first
            match (&a.file_type, &b.file_type) {
                (FileType::Directory, FileType::File) => return std::cmp::Ordering::Less,
                (FileType::File, FileType::Directory) => return std::cmp::Ordering::Greater,
                _ => {}
            }

            let cmp = match self.sort_by {
                SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Modified => a.modified.cmp(&b.modified),
            };

            if self.sort_ascending { cmp } else { cmp.reverse() }
        });
    }

    fn render_list_view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <table class="file-table">
                <thead>
                    <tr class="file-table-head">
                        <th class="sortable" onclick={ctx.link().callback(|_| FileExplorerMsg::SortBy(SortBy::Name))}>
                            { "Name" } { self.sort_indicator(SortBy::Name) }
                        </th>
                        <th class="sortable col-size" onclick={ctx.link().callback(|_| FileExplorerMsg::SortBy(SortBy::Size))}>
                            { "Size" } { self.sort_indicator(SortBy::Size) }
                        </th>
                        <th class="sortable col-modified" onclick={ctx.link().callback(|_| FileExplorerMsg::SortBy(SortBy::Modified))}>
                            { "Modified" } { self.sort_indicator(SortBy::Modified) }
                        </th>
                        <th class="col-actions">{ "" }</th>
                    </tr>
                </thead>
                <tbody>
                    {
                        self.files.iter().map(|file| {
                            let name = file.name.clone();
                            let name2 = name.clone();
                            let name3 = name.clone();
                            let is_selected = self.selected_file.as_ref() == Some(&name);
                            
                            let icon = match file.file_type {
                                FileType::Directory => "📁",
                                FileType::File => self.get_file_icon(&name),
                            };
                            
                            let size_str = match file.file_type {
                                FileType::Directory => "—".to_string(),
                                FileType::File => self.format_size(file.size),
                            };
                            
                            let date = js_sys::Date::new(&JsValue::from_f64(file.modified as f64));
                            let date_str = format!(
                                "{}/{}/{} {:02}:{:02}",
                                date.get_month() + 1,
                                date.get_date(),
                                date.get_full_year(),
                                date.get_hours(),
                                date.get_minutes()
                            );
                            
                            html! {
                                <tr
                                    class={classes!("file-row", is_selected.then_some("selected"))}
                                    onclick={ctx.link().callback(move |_| FileExplorerMsg::SelectFile(name.clone()))}
                                    ondblclick={ctx.link().callback(move |_| FileExplorerMsg::OpenFile(name2.clone()))}
                                >
                                    <td class="file-cell-name">
                                        <span class="file-cell-icon">{ icon }</span>
                                        { &file.name }
                                    </td>
                                    <td class="file-cell-meta">{ size_str }</td>
                                    <td class="file-cell-meta">{ date_str }</td>
                                    <td class="file-cell-actions">
                                        <button 
                                            class="btn btn-danger btn-small"
                                            onclick={ctx.link().callback(move |e: MouseEvent| {
                                                e.stop_propagation();
                                                FileExplorerMsg::DeleteFile(name3.clone())
                                            })}
                                        >
                                            { "Delete" }
                                        </button>
                                    </td>
                                </tr>
                            }
                        }).collect::<Html>()
                    }
                </tbody>
            </table>
        }
    }

    fn render_grid_view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="file-grid">
                {
                    self.files.iter().map(|file| {
                        let name = file.name.clone();
                        let name2 = name.clone();
                        let is_selected = self.selected_file.as_ref() == Some(&name);
                        
                        let icon = match file.file_type {
                            FileType::Directory => "📁",
                            FileType::File => self.get_file_icon(&name),
                        };
                        
                        html! {
                            <div
                                class={classes!("file-grid-item", is_selected.then_some("selected"))}
                                onclick={ctx.link().callback(move |_| FileExplorerMsg::SelectFile(name.clone()))}
                                ondblclick={ctx.link().callback(move |_| FileExplorerMsg::OpenFile(name2.clone()))}
                            >
                                <span class="file-grid-icon">{ icon }</span>
                                <span class="file-grid-label">
                                    { &file.name }
                                </span>
                            </div>
                        }
                    }).collect::<Html>()
                }
            </div>
        }
    }

    fn sort_indicator(&self, sort_by: SortBy) -> &'static str {
        if self.sort_by == sort_by {
            if self.sort_ascending { " ▲" } else { " ▼" }
        } else {
            ""
        }
    }

    fn get_file_icon(&self, name: &str) -> &'static str {
        let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
        match ext.as_str() {
            "txt" | "md" | "readme" => "📄",
            "rs" | "js" | "ts" | "py" | "c" | "cpp" | "h" | "java" => "📜",
            "html" | "css" | "scss" => "🌐",
            "json" | "yaml" | "yml" | "toml" | "xml" => "⚙️",
            "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" => "🖼️",
            "mp3" | "wav" | "ogg" | "flac" => "🎵",
            "mp4" | "mkv" | "avi" | "mov" => "🎬",
            "zip" | "tar" | "gz" | "rar" | "7z" => "📦",
            "pdf" => "📕",
            "doc" | "docx" => "📘",
            "xls" | "xlsx" => "📊",
            _ => "📄",
        }
    }

    fn format_size(&self, bytes: usize) -> String {
        if bytes < 1024 {
            format!("{} B", bytes)
        } else if bytes < 1024 * 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.0)
        } else {
            format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
        }
    }
}
