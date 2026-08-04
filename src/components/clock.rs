use yew::prelude::*;
use gloo_timers::callback::Interval;

pub struct Clock {
    hours: u32,
    minutes: u32,
    seconds: u32,
    day_name: String,
    date_string: String,
    _interval: Option<Interval>,
}

pub enum ClockMsg {
    Tick,
}

impl Component for Clock {
    type Message = ClockMsg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let link = ctx.link().clone();
        let interval = Interval::new(1000, move || {
            link.send_message(ClockMsg::Tick);
        });

        let mut clock = Self {
            hours: 0,
            minutes: 0,
            seconds: 0,
            day_name: String::new(),
            date_string: String::new(),
            _interval: Some(interval),
        };
        clock.update_time();
        clock
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ClockMsg::Tick => {
                self.update_time();
                true
            }
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        let hour_rotation = (self.hours % 12) as f64 * 30.0 + (self.minutes as f64 * 0.5);
        let minute_rotation = self.minutes as f64 * 6.0;
        let second_rotation = self.seconds as f64 * 6.0;

        html! {
            <div class="clock-widget">
                // Analog clock
                <div class="clock-face">
                    // Clock face
                    <svg viewBox="0 0 200 200" class="clock-svg">
                        // Outer ring gradient
                        <defs>
                            <linearGradient id="clockGradient" x1="0%" y1="0%" x2="100%" y2="100%">
                                <stop offset="0%" style="stop-color:#4a9eff;stop-opacity:1" />
                                <stop offset="100%" style="stop-color:#e94560;stop-opacity:1" />
                            </linearGradient>
                            <filter id="glow">
                                <feGaussianBlur stdDeviation="2" result="coloredBlur"/>
                                <feMerge>
                                    <feMergeNode in="coloredBlur"/>
                                    <feMergeNode in="SourceGraphic"/>
                                </feMerge>
                            </filter>
                        </defs>
                        
                        // Clock background
                        <circle cx="100" cy="100" r="95" fill="#1a1a2e" stroke="url(#clockGradient)" stroke-width="3"/>
                        
                        // Hour markers
                        {
                            (0..12).map(|i| {
                                let angle = (i as f64 * 30.0 - 90.0) * std::f64::consts::PI / 180.0;
                                let x1 = 100.0 + 75.0 * angle.cos();
                                let y1 = 100.0 + 75.0 * angle.sin();
                                let x2 = 100.0 + 85.0 * angle.cos();
                                let y2 = 100.0 + 85.0 * angle.sin();
                                html! {
                                    <line 
                                        x1={x1.to_string()} y1={y1.to_string()} 
                                        x2={x2.to_string()} y2={y2.to_string()} 
                                        stroke={if i % 3 == 0 { "#4a9eff" } else { "#666" }}
                                        stroke-width={if i % 3 == 0 { "3" } else { "1" }}
                                        stroke-linecap="round"
                                    />
                                }
                            }).collect::<Html>()
                        }
                        
                        // Hour hand
                        <line 
                            x1="100" y1="100" 
                            x2="100" y2="50" 
                            stroke="#ffffff" 
                            stroke-width="4" 
                            stroke-linecap="round"
                            transform={format!("rotate({} 100 100)", hour_rotation)}
                            filter="url(#glow)"
                        />
                        
                        // Minute hand
                        <line 
                            x1="100" y1="100" 
                            x2="100" y2="30" 
                            stroke="#4a9eff" 
                            stroke-width="3" 
                            stroke-linecap="round"
                            transform={format!("rotate({} 100 100)", minute_rotation)}
                            filter="url(#glow)"
                        />
                        
                        // Second hand
                        <line 
                            x1="100" y1="100" 
                            x2="100" y2="25" 
                            stroke="#e94560" 
                            stroke-width="2" 
                            stroke-linecap="round"
                            transform={format!("rotate({} 100 100)", second_rotation)}
                        />
                        
                        // Center dot
                        <circle cx="100" cy="100" r="6" fill="#e94560"/>
                        <circle cx="100" cy="100" r="3" fill="#ffffff"/>
                    </svg>
                </div>
                
                // Digital time
                <div class="clock-readout">
                    <div class="clock-time">
                        { format!("{:02}:{:02}:{:02}", self.hours, self.minutes, self.seconds) }
                    </div>
                    <div class="clock-date">
                        { &self.day_name }
                    </div>
                    <div class="clock-zone">
                        { &self.date_string }
                    </div>
                </div>
            </div>
        }
    }
}

impl Clock {
    fn update_time(&mut self) {
        let date = js_sys::Date::new_0();
        
        self.hours = date.get_hours();
        self.minutes = date.get_minutes();
        self.seconds = date.get_seconds();
        
        let days = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
        let months = ["January", "February", "March", "April", "May", "June", 
                      "July", "August", "September", "October", "November", "December"];
        
        self.day_name = days[date.get_day() as usize].to_string();
        self.date_string = format!(
            "{} {}, {}",
            months[date.get_month() as usize],
            date.get_date(),
            date.get_full_year()
        );
    }
}
