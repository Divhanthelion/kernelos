use yew::prelude::*;
use rand::RngExt;
use web_sys::MouseEvent;

const GRID_SIZE: usize = 10;
const MINE_COUNT: usize = 15;

#[derive(Clone, Copy, PartialEq)]
enum CellState {
    Hidden,
    Revealed,
    Flagged,
}

#[derive(Clone, Copy)]
struct Cell {
    is_mine: bool,
    state: CellState,
    adjacent_mines: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            is_mine: false,
            state: CellState::Hidden,
            adjacent_mines: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum GameState {
    Playing,
    Won,
    Lost,
}

pub struct Minesweeper {
    grid: Vec<Vec<Cell>>,
    game_state: GameState,
    flags_remaining: i32,
    time_elapsed: u32,
    first_click: bool,
}

pub enum MinesweeperMsg {
    LeftClick(usize, usize),
    RightClick(usize, usize, MouseEvent),
    NewGame,
    Tick,
}

impl Component for Minesweeper {
    type Message = MinesweeperMsg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        let mut game = Self {
            grid: vec![vec![Cell::default(); GRID_SIZE]; GRID_SIZE],
            game_state: GameState::Playing,
            flags_remaining: MINE_COUNT as i32,
            time_elapsed: 0,
            first_click: true,
        };
        game.initialize_grid(None);
        game
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            MinesweeperMsg::LeftClick(row, col) => {
                if self.game_state != GameState::Playing {
                    return false;
                }
                
                if self.first_click {
                    // Regenerate grid ensuring first click is safe
                    self.initialize_grid(Some((row, col)));
                    self.first_click = false;
                }
                
                self.reveal_cell(row, col);
                self.check_win();
                true
            }
            MinesweeperMsg::RightClick(row, col, event) => {
                event.prevent_default();
                
                if self.game_state != GameState::Playing {
                    return false;
                }
                
                let cell = &mut self.grid[row][col];
                match cell.state {
                    CellState::Hidden => {
                        if self.flags_remaining > 0 {
                            cell.state = CellState::Flagged;
                            self.flags_remaining -= 1;
                        }
                    }
                    CellState::Flagged => {
                        cell.state = CellState::Hidden;
                        self.flags_remaining += 1;
                    }
                    CellState::Revealed => {}
                }
                self.check_win();
                true
            }
            MinesweeperMsg::NewGame => {
                self.grid = vec![vec![Cell::default(); GRID_SIZE]; GRID_SIZE];
                self.game_state = GameState::Playing;
                self.flags_remaining = MINE_COUNT as i32;
                self.time_elapsed = 0;
                self.first_click = true;
                self.initialize_grid(None);
                true
            }
            MinesweeperMsg::Tick => {
                if self.game_state == GameState::Playing && !self.first_click {
                    self.time_elapsed += 1;
                }
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let face = match self.game_state {
            GameState::Playing => "🙂",
            GameState::Won => "😎",
            GameState::Lost => "😵",
        };

        html! {
            <div class="game-container">
                // Header
                <div class="game-header">
                    // Mines counter
                    <div class="game-score">
                        { format!("{:03}", self.flags_remaining.max(0)) }
                    </div>
                    
                    // New game button
                    <button 
                        class="game-reset"
                        onclick={ctx.link().callback(|_| MinesweeperMsg::NewGame)}
                        title="New Game"
                    >
                        { face }
                    </button>
                    
                    // Timer
                    <div class="game-score">
                        { format!("{:03}", self.time_elapsed.min(999)) }
                    </div>
                </div>
                
                // Game board
                <div class="game-board-frame">
                    <div class="game-board" style={format!("grid-template-columns: repeat({}, 1fr);", GRID_SIZE)}>
                        {
                            (0..GRID_SIZE).map(|row| {
                                (0..GRID_SIZE).map(|col| {
                                    self.render_cell(ctx, row, col)
                                }).collect::<Html>()
                            }).collect::<Html>()
                        }
                    </div>
                </div>
                
                // Instructions
                <div class="game-controls">
                    { "Left click to reveal • Right click to flag" }
                </div>
                
                // Game over message
                {
                    if self.game_state != GameState::Playing {
                        html! {
                            <div class="game-banner">
                                {
                                    match self.game_state {
                                        GameState::Won => "🎉 You Win!",
                                        GameState::Lost => "💥 Game Over!",
                                        _ => ""
                                    }
                                }
                            </div>
                        }
                    } else {
                        html! {}
                    }
                }
            </div>
        }
    }
}

impl Minesweeper {
    fn initialize_grid(&mut self, safe_cell: Option<(usize, usize)>) {
        // Reset grid
        for row in &mut self.grid {
            for cell in row {
                *cell = Cell::default();
            }
        }
        
        // Place mines
        let mut rng = rand::rng();
        let mut mines_placed = 0;

        while mines_placed < MINE_COUNT {
            let row = rng.random_range(0..GRID_SIZE);
            let col = rng.random_range(0..GRID_SIZE);
            
            // Don't place mine on the safe cell or if already a mine
            if let Some((safe_row, safe_col)) = safe_cell {
                if (row as i32 - safe_row as i32).abs() <= 1 && 
                   (col as i32 - safe_col as i32).abs() <= 1 {
                    continue;
                }
            }
            
            if !self.grid[row][col].is_mine {
                self.grid[row][col].is_mine = true;
                mines_placed += 1;
            }
        }
        
        // Calculate adjacent mines
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                if !self.grid[row][col].is_mine {
                    self.grid[row][col].adjacent_mines = self.count_adjacent_mines(row, col);
                }
            }
        }
    }

    fn count_adjacent_mines(&self, row: usize, col: usize) -> u8 {
        let mut count = 0;
        
        for dr in -1i32..=1 {
            for dc in -1i32..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                
                let new_row = row as i32 + dr;
                let new_col = col as i32 + dc;
                
                if new_row >= 0 && new_row < GRID_SIZE as i32 &&
                   new_col >= 0 && new_col < GRID_SIZE as i32 {
                    if self.grid[new_row as usize][new_col as usize].is_mine {
                        count += 1;
                    }
                }
            }
        }
        
        count
    }

    fn reveal_cell(&mut self, row: usize, col: usize) {
        let cell_state = self.grid[row][col].state;
        let cell_is_mine = self.grid[row][col].is_mine;

        if cell_state != CellState::Hidden {
            return;
        }

        self.grid[row][col].state = CellState::Revealed;

        if cell_is_mine {
            self.game_state = GameState::Lost;
            self.reveal_all_mines();
            return;
        }

        // Auto-reveal empty cells
        if self.grid[row][col].adjacent_mines == 0 {
            for dr in -1i32..=1 {
                for dc in -1i32..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    
                    let new_row = row as i32 + dr;
                    let new_col = col as i32 + dc;
                    
                    if new_row >= 0 && new_row < GRID_SIZE as i32 &&
                       new_col >= 0 && new_col < GRID_SIZE as i32 {
                        self.reveal_cell(new_row as usize, new_col as usize);
                    }
                }
            }
        }
    }

    fn reveal_all_mines(&mut self) {
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                if self.grid[row][col].is_mine {
                    self.grid[row][col].state = CellState::Revealed;
                }
            }
        }
    }

    fn check_win(&mut self) {
        if self.game_state != GameState::Playing {
            return;
        }
        
        let mut all_safe_revealed = true;
        
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                let cell = &self.grid[row][col];
                if !cell.is_mine && cell.state != CellState::Revealed {
                    all_safe_revealed = false;
                    break;
                }
            }
            if !all_safe_revealed {
                break;
            }
        }
        
        if all_safe_revealed {
            self.game_state = GameState::Won;
        }
    }

    fn render_cell(&self, ctx: &Context<Self>, row: usize, col: usize) -> Html {
        let cell = &self.grid[row][col];
        
        // Appearance is carried by classes; the adjacency digit picks a
        // `.game-cell.n1`..`.n8` colour defined in styles.css.
        let (content, state_class) = match cell.state {
            CellState::Hidden => (String::new(), "hidden".to_string()),
            CellState::Flagged => ("🚩".to_string(), "flagged".to_string()),
            CellState::Revealed => {
                if cell.is_mine {
                    ("💣".to_string(), "mine".to_string())
                } else if cell.adjacent_mines > 0 {
                    (
                        cell.adjacent_mines.to_string(),
                        format!("revealed n{}", cell.adjacent_mines),
                    )
                } else {
                    (String::new(), "revealed".to_string())
                }
            }
        };

        let on_click = ctx.link().callback(move |_| MinesweeperMsg::LeftClick(row, col));
        let on_context_menu = ctx.link().callback(move |e: MouseEvent| MinesweeperMsg::RightClick(row, col, e));

        html! {
            <div
                class={classes!("game-cell", state_class)}
                onclick={on_click}
                oncontextmenu={on_context_menu}
            >
                { content }
            </div>
        }
    }
}
