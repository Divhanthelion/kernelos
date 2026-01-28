// KernelOS v2 - A WebAssembly Desktop Environment
// Built with Rust and Yew

pub mod filesystem;
pub mod components;

use wasm_bindgen::prelude::*;
use yew::prelude::*;
use components::Desktop;

#[wasm_bindgen(start)]
pub fn run_app() {
    // Initialize logger for debugging
    wasm_logger::init(wasm_logger::Config::default());
    log::info!("KernelOS v2 starting...");
    
    // Mount the app
    let document = web_sys::window()
        .expect("No window")
        .document()
        .expect("No document");
    
    // Create a mount point if it doesn't exist
    let body = document.body().expect("No body");
    
    let mount_point = document.create_element("div").expect("Failed to create element");
    mount_point.set_id("app");
    body.append_child(&mount_point).expect("Failed to append mount point");
    
    // Render the desktop
    yew::Renderer::<Desktop>::with_root(mount_point).render();
    
    log::info!("KernelOS v2 initialized successfully!");
}
