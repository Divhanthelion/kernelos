use yew::prelude::*;
use web_sys::KeyboardEvent;
use wasm_bindgen::JsCast;

pub struct Calculator {
    display: String,
    expression: String,
    last_operator: Option<char>,
    new_number: bool,
    memory: f64,
    history: Vec<String>,
}

#[derive(Clone)]
pub enum CalculatorMsg {
    Input(char),
    Clear,
    ClearEntry,
    Equals,
    Backspace,
    ToggleSign,
    Percent,
    MemoryClear,
    MemoryRecall,
    MemoryAdd,
    MemorySubtract,
    KeyDown(KeyboardEvent),
}

impl Component for Calculator {
    type Message = CalculatorMsg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            display: "0".to_string(),
            expression: String::new(),
            last_operator: None,
            new_number: true,
            memory: 0.0,
            history: Vec::new(),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            CalculatorMsg::Input(c) => {
                match c {
                    '0'..='9' => {
                        if self.new_number {
                            self.display = c.to_string();
                            self.new_number = false;
                        } else if self.display.len() < 15 {
                            if self.display == "0" {
                                self.display = c.to_string();
                            } else {
                                self.display.push(c);
                            }
                        }
                    }
                    '.' => {
                        if self.new_number {
                            self.display = "0.".to_string();
                            self.new_number = false;
                        } else if !self.display.contains('.') {
                            self.display.push('.');
                        }
                    }
                    '+' | '-' | '×' | '÷' => {
                        self.handle_operator(c);
                    }
                    _ => {}
                }
                true
            }
            CalculatorMsg::Clear => {
                self.display = "0".to_string();
                self.expression.clear();
                self.last_operator = None;
                self.new_number = true;
                true
            }
            CalculatorMsg::ClearEntry => {
                self.display = "0".to_string();
                self.new_number = true;
                true
            }
            CalculatorMsg::Equals => {
                self.calculate();
                true
            }
            CalculatorMsg::Backspace => {
                if !self.new_number && self.display.len() > 1 {
                    self.display.pop();
                } else {
                    self.display = "0".to_string();
                    self.new_number = true;
                }
                true
            }
            CalculatorMsg::ToggleSign => {
                if self.display != "0" {
                    if self.display.starts_with('-') {
                        self.display = self.display[1..].to_string();
                    } else {
                        self.display = format!("-{}", self.display);
                    }
                }
                true
            }
            CalculatorMsg::Percent => {
                if let Ok(value) = self.display.parse::<f64>() {
                    self.display = self.format_number(value / 100.0);
                }
                true
            }
            CalculatorMsg::MemoryClear => {
                self.memory = 0.0;
                true
            }
            CalculatorMsg::MemoryRecall => {
                self.display = self.format_number(self.memory);
                self.new_number = true;
                true
            }
            CalculatorMsg::MemoryAdd => {
                if let Ok(value) = self.display.parse::<f64>() {
                    self.memory += value;
                }
                self.new_number = true;
                true
            }
            CalculatorMsg::MemorySubtract => {
                if let Ok(value) = self.display.parse::<f64>() {
                    self.memory -= value;
                }
                self.new_number = true;
                true
            }
            CalculatorMsg::KeyDown(event) => {
                let msg = match event.key().as_str() {
                    "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
                        let c = event.key().chars().next().unwrap();
                        Some(CalculatorMsg::Input(c))
                    }
                    "." | "," => Some(CalculatorMsg::Input('.')),
                    "+" => Some(CalculatorMsg::Input('+')),
                    "-" => Some(CalculatorMsg::Input('-')),
                    "*" => Some(CalculatorMsg::Input('×')),
                    "/" => Some(CalculatorMsg::Input('÷')),
                    "Enter" | "=" => Some(CalculatorMsg::Equals),
                    "Escape" => Some(CalculatorMsg::Clear),
                    "Backspace" => Some(CalculatorMsg::Backspace),
                    _ => None,
                };
                if let Some(m) = msg {
                    _ctx.link().send_message(m);
                }
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let onkeydown = ctx.link().callback(CalculatorMsg::KeyDown);

        html! {
            <div 
                class="calculator" 
                style="display: flex; flex-direction: column; height: 100%; background: linear-gradient(180deg, #1a1a1a 0%, #2d2d2d 100%); padding: 16px;"
                tabindex="0"
                {onkeydown}
            >
                // Display
                <div style="background: linear-gradient(180deg, #0a0a0a 0%, #1a1a1a 100%); border-radius: 12px; padding: 16px; margin-bottom: 16px; box-shadow: inset 0 2px 8px rgba(0,0,0,0.5);">
                    <div style="color: #888; font-size: 14px; min-height: 20px; text-align: right; margin-bottom: 4px; overflow: hidden; text-overflow: ellipsis;">
                        { &self.expression }
                    </div>
                    <div style="color: white; font-size: 42px; font-weight: 300; text-align: right; overflow: hidden; text-overflow: ellipsis; font-family: 'Segoe UI', sans-serif;">
                        { &self.display }
                    </div>
                </div>
                
                // Memory indicator
                {
                    if self.memory != 0.0 {
                        html! {
                            <div style="color: #4a9eff; font-size: 11px; margin-bottom: 8px; padding-left: 4px;">
                                { format!("M: {}", self.format_number(self.memory)) }
                            </div>
                        }
                    } else {
                        html! {}
                    }
                }
                
                // Buttons
                <div style="display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; flex: 1;">
                    // Row 1: Memory functions
                    { self.render_button(ctx, "MC", "memory", CalculatorMsg::MemoryClear) }
                    { self.render_button(ctx, "MR", "memory", CalculatorMsg::MemoryRecall) }
                    { self.render_button(ctx, "M+", "memory", CalculatorMsg::MemoryAdd) }
                    { self.render_button(ctx, "M−", "memory", CalculatorMsg::MemorySubtract) }
                    
                    // Row 2
                    { self.render_button(ctx, "C", "function", CalculatorMsg::Clear) }
                    { self.render_button(ctx, "CE", "function", CalculatorMsg::ClearEntry) }
                    { self.render_button(ctx, "%", "function", CalculatorMsg::Percent) }
                    { self.render_button(ctx, "÷", "operator", CalculatorMsg::Input('÷')) }
                    
                    // Row 3
                    { self.render_button(ctx, "7", "number", CalculatorMsg::Input('7')) }
                    { self.render_button(ctx, "8", "number", CalculatorMsg::Input('8')) }
                    { self.render_button(ctx, "9", "number", CalculatorMsg::Input('9')) }
                    { self.render_button(ctx, "×", "operator", CalculatorMsg::Input('×')) }
                    
                    // Row 4
                    { self.render_button(ctx, "4", "number", CalculatorMsg::Input('4')) }
                    { self.render_button(ctx, "5", "number", CalculatorMsg::Input('5')) }
                    { self.render_button(ctx, "6", "number", CalculatorMsg::Input('6')) }
                    { self.render_button(ctx, "−", "operator", CalculatorMsg::Input('-')) }
                    
                    // Row 5
                    { self.render_button(ctx, "1", "number", CalculatorMsg::Input('1')) }
                    { self.render_button(ctx, "2", "number", CalculatorMsg::Input('2')) }
                    { self.render_button(ctx, "3", "number", CalculatorMsg::Input('3')) }
                    { self.render_button(ctx, "+", "operator", CalculatorMsg::Input('+')) }
                    
                    // Row 6
                    { self.render_button(ctx, "±", "function", CalculatorMsg::ToggleSign) }
                    { self.render_button(ctx, "0", "number", CalculatorMsg::Input('0')) }
                    { self.render_button(ctx, ".", "number", CalculatorMsg::Input('.')) }
                    { self.render_button(ctx, "=", "equals", CalculatorMsg::Equals) }
                </div>
            </div>
        }
    }
}

impl Calculator {
    fn render_button(&self, ctx: &Context<Self>, label: &str, btn_type: &str, msg: CalculatorMsg) -> Html {
        let (bg_color, hover_color, text_color) = match btn_type {
            "number" => ("#3d3d3d", "#4d4d4d", "#ffffff"),
            "operator" => ("#ff9500", "#ffaa33", "#ffffff"),
            "function" => ("#505050", "#606060", "#ffffff"),
            "memory" => ("#2d2d2d", "#3d3d3d", "#4a9eff"),
            "equals" => ("#4a9eff", "#5aaeFF", "#ffffff"),
            _ => ("#3d3d3d", "#4d4d4d", "#ffffff"),
        };

        html! {
            <button 
                style={format!(
                    "border: none; border-radius: 12px; font-size: 20px; cursor: pointer; \
                     background-color: {}; color: {}; transition: all 0.15s ease; \
                     box-shadow: 0 2px 4px rgba(0,0,0,0.2); \
                     display: flex; align-items: center; justify-content: center;",
                    bg_color, text_color
                )}
                onclick={ctx.link().callback(move |_| msg.clone())}
                onmouseover={Callback::from({
                    let hover = hover_color.to_string();
                    move |e: MouseEvent| {
                        if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                            let _ = el.style().set_property("background-color", &hover);
                            let _ = el.style().set_property("transform", "scale(0.98)");
                        }
                    }
                })}
                onmouseout={Callback::from({
                    let bg = bg_color.to_string();
                    move |e: MouseEvent| {
                        if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                            let _ = el.style().set_property("background-color", &bg);
                            let _ = el.style().set_property("transform", "scale(1)");
                        }
                    }
                })}
            >
                { label }
            </button>
        }
    }

    fn handle_operator(&mut self, op: char) {
        if !self.expression.is_empty() && self.last_operator.is_some() && !self.new_number {
            self.calculate();
        }
        
        self.expression = format!("{} {} ", self.display, op);
        self.last_operator = Some(op);
        self.new_number = true;
    }

    fn calculate(&mut self) {
        if self.expression.is_empty() || self.last_operator.is_none() {
            return;
        }
        
        let parts: Vec<&str> = self.expression.trim().split_whitespace().collect();
        if parts.len() < 2 {
            return;
        }
        
        let first: f64 = match parts[0].parse() {
            Ok(n) => n,
            Err(_) => return,
        };
        
        let op = parts[1].chars().next().unwrap_or('+');
        let second: f64 = match self.display.parse() {
            Ok(n) => n,
            Err(_) => return,
        };
        
        let result = match op {
            '+' => first + second,
            '-' | '−' => first - second,
            '×' | '*' => first * second,
            '÷' | '/' => {
                if second == 0.0 {
                    self.display = "Error".to_string();
                    self.expression.clear();
                    self.last_operator = None;
                    self.new_number = true;
                    return;
                }
                first / second
            }
            _ => return,
        };
        
        // Store in history
        self.history.push(format!("{} {} {} = {}", first, op, second, result));
        if self.history.len() > 10 {
            self.history.remove(0);
        }
        
        self.display = self.format_number(result);
        self.expression.clear();
        self.last_operator = None;
        self.new_number = true;
    }

    fn format_number(&self, n: f64) -> String {
        if n.is_nan() || n.is_infinite() {
            return "Error".to_string();
        }
        
        // Handle very large or small numbers
        if n.abs() >= 1e15 || (n.abs() < 1e-10 && n != 0.0) {
            return format!("{:.6e}", n);
        }
        
        // Remove trailing zeros
        let s = format!("{:.10}", n);
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        
        // Limit length
        if s.len() > 15 {
            format!("{:.10e}", n)
        } else {
            s.to_string()
        }
    }
}
