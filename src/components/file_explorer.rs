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
enum SortBy {
    Name,
    Size,
    Modified,
    Type,
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
            <div class="file-explorer" style="display: flex; flex-direction: column; height: 100%; background-color: #252526;">
                // Toolbar
                <div style="display: flex; align-items: center; padding: 8px; gap: 8px; border-bottom: 1px solid #333; background-color: #2d2d2d;">
                    <button 
                        style="padding: 6px 12px; border: none; border-radius: 4px; background-color: #3c3c3c; color: white; cursor: pointer;"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::NavigateUp)}
                    >
                        { "↑ Up" }
                    </button>
                    <button 
                        style="padding: 6px 12px; border: none; border-radius: 4px; background-color: #3c3c3c; color: white; cursor: pointer;"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::Refresh)}
                    >
                        { "⟳" }
                    </button>
                    <div style="flex: 1;" />
                    <button 
                        style="padding: 6px 12px; border: none; border-radius: 4px; background-color: #4a9eff; color: white; cursor: pointer;"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::CreateNewFile)}
                    >
                        { "+ File" }
                    </button>
                    <button 
                        style="padding: 6px 12px; border: none; border-radius: 4px; background-color: #4a9eff; color: white; cursor: pointer;"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::CreateNewDirectory)}
                    >
                        { "+ Folder" }
                    </button>
                    <button 
                        style="padding: 6px 12px; border: none; border-radius: 4px; background-color: #3c3c3c; color: white; cursor: pointer;"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::ToggleViewMode)}
                    >
                        { if self.view_mode == ViewMode::List { "☷" } else { "☰" } }
                    </button>
                </div>
                
                // Path bar
                <div style="display: flex; align-items: center; padding: 8px; background-color: #3c3c3c; gap: 4px;">
                    <button 
                        style="padding: 4px 8px; border: none; border-radius: 4px; background-color: transparent; color: #4a9eff; cursor: pointer;"
                        onclick={ctx.link().callback(|_| FileExplorerMsg::NavigateTo("/".to_string()))}
                    >
                        { "/" }
                    </button>
                    {
                        path_parts.iter().enumerate().map(|(i, part)| {
                            let path = format!("/{}", path_parts[0..=i].join("/"));
                            html! {
                                <>
                                    <span style="color: #666;">{ "›" }</span>
                                    <button 
                                        style="padding: 4px 8px; border: none; border-radius: 4px; background-color: transparent; color: #4a9eff; cursor: pointer;"
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
                            <div style="padding: 8px; background-color: #5a1d1d; color: #ff6b6b; display: flex; justify-content: space-between; align-items: center;">
                                <span>{ error }</span>
                                <button 
                                    style="background: none; border: none; color: #ff6b6b; cursor: pointer;"
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
                <div style="flex: 1; overflow-y: auto; padding: 8px;">
                    {
                        if self.view_mode == ViewMode::List {
                            self.render_list_view(ctx)
                        } else {
                            self.render_grid_view(ctx)
                        }
                    }
                </div>
                
                // Status bar
                <div style="padding: 8px; border-top: 1px solid #333; background-color: #2d2d2d; color: #888; font-size: 12px;">
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
            let dir_cmp = match (&a.file_type, &b.file_type) {
                (FileType::Directory, FileType::File) => return std::cmp::Ordering::Less,
                (FileType::File, FileType::Directory) => return std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            };
            
            let cmp = match self.sort_by {
                SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Modified => a.modified.cmp(&b.modified),
                SortBy::Type => {
                    let ext_a = a.name.rsplit('.').next().unwrap_or("");
                    let ext_b = b.name.rsplit('.').next().unwrap_or("");
                    ext_a.cmp(ext_b)
                }
            };
            
            if self.sort_ascending { cmp } else { cmp.reverse() }
        });
    }

    fn render_list_view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <table style="width: 100%; border-collapse: collapse;">
                <thead>
                    <tr style="color: #888; font-size: 12px; text-align: left;">
                        <th style="padding: 8px; cursor: pointer;" onclick={ctx.link().callback(|_| FileExplorerMsg::SortBy(SortBy::Name))}>
                            { "Name" } { self.sort_indicator(SortBy::Name) }
                        </th>
                        <th style="padding: 8px; width: 100px; cursor: pointer;" onclick={ctx.link().callback(|_| FileExplorerMsg::SortBy(SortBy::Size))}>
                            { "Size" } { self.sort_indicator(SortBy::Size) }
                        </th>
                        <th style="padding: 8px; width: 150px; cursor: pointer;" onclick={ctx.link().callback(|_| FileExplorerMsg::SortBy(SortBy::Modified))}>
                            { "Modified" } { self.sort_indicator(SortBy::Modified) }
                        </th>
                        <th style="padding: 8px; width: 80px;">{ "" }</th>
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
                                    style={format!(
                                        "cursor: pointer; {}",
                                        if is_selected { "background-color: rgba(74, 158, 255, 0.2);" } else { "" }
                                    )}
                                    onclick={ctx.link().callback(move |_| FileExplorerMsg::SelectFile(name.clone()))}
                                    ondblclick={ctx.link().callback(move |_| FileExplorerMsg::OpenFile(name2.clone()))}
                                >
                                    <td style="padding: 8px; color: #d4d4d4;">
                                        <span style="margin-right: 8px;">{ icon }</span>
                                        { &file.name }
                                    </td>
                                    <td style="padding: 8px; color: #888;">{ size_str }</td>
                                    <td style="padding: 8px; color: #888;">{ date_str }</td>
                                    <td style="padding: 8px;">
                                        <button 
                                            style="padding: 4px 8px; border: none; border-radius: 4px; background-color: #5a1d1d; color: #ff6b6b; cursor: pointer; font-size: 11px;"
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
            <div style="display: grid; grid-template-columns: repeat(auto-fill, minmax(100px, 1fr)); gap: 16px; padding: 8px;">
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
                                style={format!(
                                    "display: flex; flex-direction: column; align-items: center; padding: 12px; border-radius: 8px; cursor: pointer; {}",
                                    if is_selected { "background-color: rgba(74, 158, 255, 0.2);" } else { "" }
                                )}
                                onclick={ctx.link().callback(move |_| FileExplorerMsg::SelectFile(name.clone()))}
                                ondblclick={ctx.link().callback(move |_| FileExplorerMsg::OpenFile(name2.clone()))}
                            >
                                <span style="font-size: 48px; margin-bottom: 8px;">{ icon }</span>
                                <span style="color: #d4d4d4; font-size: 12px; text-align: center; word-break: break-word; max-width: 90px;">
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
