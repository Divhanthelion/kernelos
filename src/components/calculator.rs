use yew::prelude::*;
use web_sys::KeyboardEvent;

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
                tabindex="0"
                {onkeydown}
            >
                // Display
                <div class="calculator-display">
                    <div class="calculator-expression">
                        { &self.expression }
                    </div>
                    <div class="calculator-result">
                        { &self.display }
                    </div>
                </div>
                
                // Memory indicator
                {
                    if self.memory != 0.0 {
                        html! {
                            <div class="calculator-memory">
                                { format!("M: {}", self.format_number(self.memory)) }
                            </div>
                        }
                    } else {
                        html! {}
                    }
                }
                
                // Buttons
                <div class="calculator-buttons">
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
    /// Hover and press states are CSS (`.calculator-button:hover`), not
    /// hand-rolled JS style mutation.
    fn render_button(&self, ctx: &Context<Self>, label: &str, btn_type: &str, msg: CalculatorMsg) -> Html {
        html! {
            <button
                class={classes!("calculator-button", btn_type.to_string())}
                onclick={ctx.link().callback(move |_| msg.clone())}
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
