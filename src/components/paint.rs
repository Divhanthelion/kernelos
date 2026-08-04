use yew::prelude::*;
use web_sys::{HtmlCanvasElement, CanvasRenderingContext2d, MouseEvent};
use wasm_bindgen::JsCast;

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Brush,
    Eraser,
    Line,
    Rectangle,
    Circle,
    Fill,
}

pub struct Paint {
    canvas_ref: NodeRef,
    is_drawing: bool,
    last_x: f64,
    last_y: f64,
    start_x: f64,
    start_y: f64,
    current_tool: Tool,
    current_color: String,
    brush_size: u32,
    canvas_width: u32,
    canvas_height: u32,
}

pub enum PaintMsg {
    MouseDown(MouseEvent),
    MouseMove(MouseEvent),
    MouseUp(MouseEvent),
    MouseLeave,
    SetTool(Tool),
    SetColor(String),
    SetBrushSize(u32),
    Clear,
    Undo,
}

impl Component for Paint {
    type Message = PaintMsg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            canvas_ref: NodeRef::default(),
            is_drawing: false,
            last_x: 0.0,
            last_y: 0.0,
            start_x: 0.0,
            start_y: 0.0,
            current_tool: Tool::Brush,
            current_color: "#000000".to_string(),
            brush_size: 5,
            canvas_width: 800,
            canvas_height: 500,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            PaintMsg::MouseDown(event) => {
                self.is_drawing = true;
                let (x, y) = self.get_canvas_coords(&event);
                self.last_x = x;
                self.last_y = y;
                self.start_x = x;
                self.start_y = y;
                
                if self.current_tool == Tool::Brush || self.current_tool == Tool::Eraser {
                    self.draw_dot(x, y);
                }
                true
            }
            PaintMsg::MouseMove(event) => {
                if !self.is_drawing {
                    return false;
                }
                
                let (x, y) = self.get_canvas_coords(&event);
                
                match self.current_tool {
                    Tool::Brush | Tool::Eraser => {
                        self.draw_line(self.last_x, self.last_y, x, y);
                        self.last_x = x;
                        self.last_y = y;
                    }
                    _ => {}
                }
                true
            }
            PaintMsg::MouseUp(event) => {
                if !self.is_drawing {
                    return false;
                }
                
                let (x, y) = self.get_canvas_coords(&event);
                
                match self.current_tool {
                    Tool::Line => self.draw_shape_line(self.start_x, self.start_y, x, y),
                    Tool::Rectangle => self.draw_rectangle(self.start_x, self.start_y, x, y),
                    Tool::Circle => self.draw_circle(self.start_x, self.start_y, x, y),
                    _ => {}
                }
                
                self.is_drawing = false;
                true
            }
            PaintMsg::MouseLeave => {
                self.is_drawing = false;
                true
            }
            PaintMsg::SetTool(tool) => {
                self.current_tool = tool;
                true
            }
            PaintMsg::SetColor(color) => {
                self.current_color = color;
                true
            }
            PaintMsg::SetBrushSize(size) => {
                self.brush_size = size;
                true
            }
            PaintMsg::Clear => {
                self.clear_canvas();
                true
            }
            PaintMsg::Undo => {
                // Simple undo by clearing (proper undo would need history)
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let onmousedown = ctx.link().callback(PaintMsg::MouseDown);
        let onmousemove = ctx.link().callback(PaintMsg::MouseMove);
        let onmouseup = ctx.link().callback(PaintMsg::MouseUp);
        let onmouseleave = ctx.link().callback(|_| PaintMsg::MouseLeave);

        let colors = vec![
            "#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", 
            "#ffff00", "#ff00ff", "#00ffff", "#ff9900", "#9900ff",
            "#663300", "#006633", "#333366", "#ff6699", "#99ff66"
        ];

        html! {
            <div class="paint">
                // Toolbar
                <div class="paint-toolbar">
                    // Tools
                    <div class="paint-toolgroup">
                        { self.render_tool_button(ctx, Tool::Brush, "✏️", "Brush") }
                        { self.render_tool_button(ctx, Tool::Eraser, "🧽", "Eraser") }
                        { self.render_tool_button(ctx, Tool::Line, "📏", "Line") }
                        { self.render_tool_button(ctx, Tool::Rectangle, "⬜", "Rectangle") }
                        { self.render_tool_button(ctx, Tool::Circle, "⭕", "Circle") }
                    </div>
                    
                    // Colors
                    <div class="paint-toolgroup">
                        {
                            colors.iter().map(|color| {
                                let c = color.to_string();
                                let is_selected = &self.current_color == *color;
                                html! {
                                    <div
                                        class={classes!("paint-color", is_selected.then_some("active"))}
                                        style={format!("background-color: {};", color)}
                                        onclick={ctx.link().callback(move |_| PaintMsg::SetColor(c.clone()))}
                                    />
                                }
                            }).collect::<Html>()
                        }
                    </div>
                    
                    // Current color display
                    <div class="paint-toolgroup">
                        <span class="paint-label">{ "Color:" }</span>
                        <div class="paint-current-color" style={format!("background-color: {};", self.current_color)} />
                    </div>
                    
                    // Brush size
                    <div class="paint-toolgroup">
                        <span class="paint-label">{ "Size:" }</span>
                        {
                            [2u32, 5, 10, 20, 40].iter().map(|size| {
                                let s = *size;
                                let is_selected = self.brush_size == s;
                                html! {
                                    <button
                                        class={classes!("paint-size", is_selected.then_some("active"))}
                                        onclick={ctx.link().callback(move |_| PaintMsg::SetBrushSize(s))}
                                    >
                                        <div class="paint-size-dot" style={format!(
                                            "width: {p}px; height: {p}px;",
                                            p = (s as f64 * 0.6).min(20.0) as u32
                                        )} />
                                    </button>
                                }
                            }).collect::<Html>()
                        }
                    </div>
                    
                    // Actions
                    <div class="paint-toolgroup end">
                        <button 
                            class="btn btn-danger"
                            onclick={ctx.link().callback(|_| PaintMsg::Clear)}
                        >
                            { "🗑️ Clear" }
                        </button>
                    </div>
                </div>
                
                // Canvas area
                <div class="paint-canvas-container">
                    <canvas 
                        ref={self.canvas_ref.clone()}
                        width={self.canvas_width.to_string()}
                        height={self.canvas_height.to_string()}
                        class="paint-canvas"
                        {onmousedown}
                        {onmousemove}
                        {onmouseup}
                        {onmouseleave}
                    />
                </div>
                
                // Status bar
                <div class="paint-status">
                    <span>{ format!("Tool: {:?}", self.current_tool) }</span>
                    <span>{ format!("Size: {}px", self.brush_size) }</span>
                    <span>{ format!("Canvas: {}×{}", self.canvas_width, self.canvas_height) }</span>
                </div>
            </div>
        }
    }

    fn rendered(&mut self, _ctx: &Context<Self>, first_render: bool) {
        if first_render {
            self.clear_canvas();
        }
    }
}

impl Paint {
    fn render_tool_button(&self, ctx: &Context<Self>, tool: Tool, icon: &str, title: &str) -> Html {
        let is_active = self.current_tool == tool;
        
        html! {
            <button
                class={classes!("paint-tool", is_active.then_some("active"))}
                onclick={ctx.link().callback(move |_| PaintMsg::SetTool(tool))}
                title={title.to_string()}
            >
                { icon }
            </button>
        }
    }

    fn get_canvas_coords(&self, event: &MouseEvent) -> (f64, f64) {
        if let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() {
            let rect = canvas.get_bounding_client_rect();
            let x = event.client_x() as f64 - rect.left();
            let y = event.client_y() as f64 - rect.top();
            return (x, y);
        }
        (0.0, 0.0)
    }

    fn get_context(&self) -> Option<CanvasRenderingContext2d> {
        self.canvas_ref.cast::<HtmlCanvasElement>()
            .and_then(|canvas| {
                canvas.get_context("2d")
                    .ok()
                    .flatten()
                    .and_then(|ctx| ctx.dyn_into::<CanvasRenderingContext2d>().ok())
            })
    }

    fn draw_dot(&self, x: f64, y: f64) {
        if let Some(ctx) = self.get_context() {
            ctx.begin_path();
            ctx.arc(x, y, self.brush_size as f64 / 2.0, 0.0, std::f64::consts::PI * 2.0).ok();
            
            if self.current_tool == Tool::Eraser {
                ctx.set_fill_style_str("white");
            } else {
                ctx.set_fill_style_str(&self.current_color);
            }
            ctx.fill();
        }
    }

    fn draw_line(&self, x1: f64, y1: f64, x2: f64, y2: f64) {
        if let Some(ctx) = self.get_context() {
            ctx.begin_path();
            ctx.set_line_cap("round");
            ctx.set_line_join("round");
            ctx.set_line_width(self.brush_size as f64);
            
            if self.current_tool == Tool::Eraser {
                ctx.set_stroke_style_str("white");
            } else {
                ctx.set_stroke_style_str(&self.current_color);
            }
            
            ctx.move_to(x1, y1);
            ctx.line_to(x2, y2);
            ctx.stroke();
        }
    }

    fn draw_shape_line(&self, x1: f64, y1: f64, x2: f64, y2: f64) {
        if let Some(ctx) = self.get_context() {
            ctx.begin_path();
            ctx.set_line_width(self.brush_size as f64);
            ctx.set_stroke_style_str(&self.current_color);
            ctx.set_line_cap("round");
            ctx.move_to(x1, y1);
            ctx.line_to(x2, y2);
            ctx.stroke();
        }
    }

    fn draw_rectangle(&self, x1: f64, y1: f64, x2: f64, y2: f64) {
        if let Some(ctx) = self.get_context() {
            let x = x1.min(x2);
            let y = y1.min(y2);
            let width = (x2 - x1).abs();
            let height = (y2 - y1).abs();
            
            ctx.begin_path();
            ctx.set_line_width(self.brush_size as f64);
            ctx.set_stroke_style_str(&self.current_color);
            ctx.stroke_rect(x, y, width, height);
        }
    }

    fn draw_circle(&self, x1: f64, y1: f64, x2: f64, y2: f64) {
        if let Some(ctx) = self.get_context() {
            let cx = (x1 + x2) / 2.0;
            let cy = (y1 + y2) / 2.0;
            let rx = (x2 - x1).abs() / 2.0;
            let ry = (y2 - y1).abs() / 2.0;
            let radius = rx.max(ry);
            
            ctx.begin_path();
            ctx.set_line_width(self.brush_size as f64);
            ctx.set_stroke_style_str(&self.current_color);
            ctx.arc(cx, cy, radius, 0.0, std::f64::consts::PI * 2.0).ok();
            ctx.stroke();
        }
    }

    fn clear_canvas(&self) {
        if let Some(ctx) = self.get_context() {
            ctx.set_fill_style_str("white");
            ctx.fill_rect(0.0, 0.0, self.canvas_width as f64, self.canvas_height as f64);
        }
    }
}

impl std::fmt::Debug for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tool::Brush => write!(f, "Brush"),
            Tool::Eraser => write!(f, "Eraser"),
            Tool::Line => write!(f, "Line"),
            Tool::Rectangle => write!(f, "Rectangle"),
            Tool::Circle => write!(f, "Circle"),
            Tool::Fill => write!(f, "Fill"),
        }
    }
}
