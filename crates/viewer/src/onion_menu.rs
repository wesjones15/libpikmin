//! The viewer's Onion window. Rules are the core's (`libpikmin::onion_menu`); the look is this
//! viewer's own (Pikmin 1 draws its window from `screen/` layouts this viewer doesn't load).
//!
//! W / up moves Pikmin out of the Onion to the squad, S / down puts squad Pikmin in, A / D
//! picks the colour of a shared Onion, Space confirms and X cancels. The sim is paused while
//! the window is open, like `gameflow.mPauseAll` in `NaviContainerState`.

use libpikmin::onion::OnionId;
use libpikmin::{CaptainId, PikiColor, Sim};
use macroquad::prelude::*;

pub struct OnionMenu {
    pub onion: OnionId,
    pub captain: CaptainId,
    color: usize,
    /// > 0: call this many out; < 0: put this many away.
    delta: i32,
    repeat: f32,
}

pub enum MenuResult {
    Open,
    Closed,
}

fn rgb(c: PikiColor) -> Color {
    match c {
        PikiColor::Red => Color::from_rgba(225, 45, 45, 255),
        PikiColor::Yellow => Color::from_rgba(245, 205, 40, 255),
        PikiColor::Blue => Color::from_rgba(55, 100, 235, 255),
        PikiColor::Purple => Color::from_rgba(150, 60, 215, 255),
        PikiColor::White => Color::from_rgba(240, 240, 240, 255),
    }
}

impl OnionMenu {
    pub fn new(onion: OnionId, captain: CaptainId) -> Self {
        OnionMenu {
            onion,
            captain,
            color: 0,
            delta: 0,
            repeat: 0.0,
        }
    }

    fn color(&self, sim: &Sim) -> PikiColor {
        let colors = &sim.onions()[self.onion.0 as usize].colors;
        colors[self.color % colors.len()]
    }

    /// Handles input and applies the result on confirm.
    pub fn update(&mut self, sim: &mut Sim, dt: f32) -> MenuResult {
        let ncolors = sim.onions()[self.onion.0 as usize].colors.len();
        if is_key_pressed(KeyCode::A) || is_key_pressed(KeyCode::Left) {
            self.color = (self.color + ncolors - 1) % ncolors;
            self.delta = 0;
        }
        if is_key_pressed(KeyCode::D) || is_key_pressed(KeyCode::Right) {
            self.color = (self.color + 1) % ncolors;
            self.delta = 0;
        }
        let counts = sim.onion_counts(self.onion, self.captain, self.color(sim));
        let up = is_key_down(KeyCode::W) || is_key_down(KeyCode::Up);
        let down = is_key_down(KeyCode::S) || is_key_down(KeyCode::Down);
        let tapped = is_key_pressed(KeyCode::W)
            || is_key_pressed(KeyCode::Up)
            || is_key_pressed(KeyCode::S)
            || is_key_pressed(KeyCode::Down);
        self.repeat -= dt;
        if (up || down) && (tapped || self.repeat <= 0.0) {
            self.repeat = if tapped { 0.35 } else { 0.06 };
            self.delta += if up { 1 } else { -1 };
        }
        self.delta = self
            .delta
            .clamp(-(counts.in_squad as i32), counts.can_call_out() as i32);
        if is_key_pressed(KeyCode::Space) || is_key_pressed(KeyCode::Enter) {
            let color = self.color(sim);
            if self.delta > 0 {
                sim.onion_call_out(self.captain, self.onion, color, self.delta as u32);
            } else if self.delta < 0 {
                sim.onion_put_away(self.captain, self.onion, color, (-self.delta) as u32);
            }
            return MenuResult::Closed;
        }
        if is_key_pressed(KeyCode::X) || is_key_pressed(KeyCode::Escape) {
            return MenuResult::Closed;
        }
        MenuResult::Open
    }

    pub fn draw(&self, sim: &Sim) {
        let color = self.color(sim);
        let c = sim.onion_counts(self.onion, self.captain, color);
        let (w, h) = (420.0, 300.0);
        let (x, y) = ((screen_width() - w) / 2.0, (screen_height() - h) / 2.0);
        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.35));
        draw_rectangle(x, y, w, h, Color::from_rgba(20, 30, 40, 235));
        draw_rectangle_lines(x, y, w, h, 3.0, rgb(color));

        let col = rgb(color);
        let name = match color {
            PikiColor::Red => "Red",
            PikiColor::Yellow => "Yellow",
            PikiColor::Blue => "Blue",
            PikiColor::Purple => "Purple",
            PikiColor::White => "White",
        };
        let ncolors = sim.onions()[self.onion.0 as usize].colors.len();
        let title = if ncolors > 1 { format!("< {name} Onion >") } else { format!("{name} Onion") };
        let tw = measure_text(&title, None, 30, 1.0).width;
        draw_text(&title, x + (w - tw) / 2.0, y + 38.0, 30.0, col);

        let onion_n = c.in_onion as i32 - self.delta;
        let squad_n = c.in_squad as i32 + self.delta;
        let big = |text: &str, cx: f32, cy: f32, color: Color| {
            let tw = measure_text(text, None, 48, 1.0).width;
            draw_text(text, cx - tw / 2.0, cy, 48.0, color);
        };
        // Onion on top, squad below, the transfer between them.
        draw_circle(x + w / 2.0, y + 95.0, 34.0, Color::new(col.r, col.g, col.b, 0.35));
        big(&onion_n.to_string(), x + w / 2.0, y + 112.0, WHITE);
        draw_text("in the Onion", x + w / 2.0 + 44.0, y + 102.0, 20.0, LIGHTGRAY);

        let arrow = match self.delta.signum() {
            1 => format!("v  {}  v", self.delta),
            -1 => format!("^  {}  ^", -self.delta),
            _ => "-".to_string(),
        };
        let aw = measure_text(&arrow, None, 28, 1.0).width;
        draw_text(&arrow, x + (w - aw) / 2.0, y + 160.0, 28.0, YELLOW);

        draw_circle(x + w / 2.0, y + 200.0, 34.0, Color::new(1.0, 1.0, 1.0, 0.12));
        big(&squad_n.to_string(), x + w / 2.0, y + 217.0, WHITE);
        draw_text("in your squad", x + w / 2.0 + 44.0, y + 207.0, 20.0, LIGHTGRAY);

        let field = format!(
            "On the field {} / {}",
            c.on_field as i32 + self.delta.max(0),
            c.max_on_field
        );
        draw_text(&field, x + 18.0, y + h - 46.0, 20.0, LIGHTGRAY);
        draw_text(
            "W/S move Pikmin   Space confirm   X cancel",
            x + 18.0,
            y + h - 18.0,
            18.0,
            GRAY,
        );
    }
}
