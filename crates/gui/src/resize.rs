use crate::view::Sizing;

pub const FREE_HEIGHT: f32 = 720.0;
pub const FIT_MIN_HEIGHT: f32 = 300.0;
pub const FREE_MIN_HEIGHT: f32 = 480.0;
pub const MAX_HEIGHT: f32 = 1600.0;
pub const RESIZE_THRESHOLD: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Resizable(bool),
    MinHeight(f32),
    Height(f32),
    Maximize(bool),
    MaximizeButton(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resizer {
    sizing: Option<Sizing>,
    free_height: f32,
    sent_height: Option<f32>,
    settling_from: Option<f32>,
    last_inner: Option<f32>,
}

impl Default for Resizer {
    fn default() -> Self {
        Self::new()
    }
}

fn apart(first: f32, second: f32) -> bool {
    (first - second).abs() >= RESIZE_THRESHOLD
}

impl Resizer {
    pub fn new() -> Self {
        Self {
            sizing: None,
            free_height: FREE_HEIGHT,
            sent_height: None,
            settling_from: None,
            last_inner: None,
        }
    }

    pub fn layout_height(&self, sizing: Sizing, inner_height: f32) -> f32 {
        let settling = self.sizing != Some(Sizing::Free) || self.settling_from.is_some();
        if sizing == Sizing::Free && settling {
            self.free_height
        } else {
            inner_height
        }
    }

    pub fn update(
        &mut self,
        sizing: Sizing,
        fit_target: f32,
        inner_height: Option<f32>,
        maximized: Option<bool>,
    ) -> Vec<Command> {
        let entered = self.sizing != Some(sizing);
        self.sizing = Some(sizing);
        let commands = match sizing {
            Sizing::Fit => self.fit(entered, fit_target, inner_height, maximized),
            Sizing::Free => self.free(entered, inner_height, maximized),
        };
        self.last_inner = inner_height;
        commands
    }

    fn fit(
        &mut self,
        entered: bool,
        fit_target: f32,
        inner_height: Option<f32>,
        maximized: Option<bool>,
    ) -> Vec<Command> {
        let target = fit_target.clamp(FIT_MIN_HEIGHT, MAX_HEIGHT);
        let mut commands = Vec::new();
        if entered {
            commands.push(Command::Resizable(false));
            commands.push(Command::MaximizeButton(false));
            commands.push(Command::MinHeight(FIT_MIN_HEIGHT));
        }
        let unmaximize = maximized == Some(true);
        if unmaximize {
            commands.push(Command::Maximize(false));
        }
        let target_moved = self.sent_height.is_none_or(|sent| apart(sent, target));
        let refit = match (inner_height, self.sent_height) {
            (Some(inner), Some(sent)) => apart(inner, sent) && self.last_inner != Some(inner),
            _ => false,
        };
        if entered || unmaximize || target_moved || refit {
            self.sent_height = Some(target);
            commands.push(Command::Height(target));
        }
        commands
    }

    fn free(
        &mut self,
        entered: bool,
        inner_height: Option<f32>,
        maximized: Option<bool>,
    ) -> Vec<Command> {
        if entered {
            self.sent_height = Some(self.free_height);
            self.settling_from = inner_height.filter(|inner| apart(*inner, self.free_height));
            return vec![
                Command::Resizable(true),
                Command::MaximizeButton(true),
                Command::MinHeight(FREE_MIN_HEIGHT),
                Command::Height(self.free_height),
            ];
        }
        let Some(inner) = inner_height else {
            return Vec::new();
        };
        if let Some(from) = self.settling_from {
            if !apart(inner, from) {
                return Vec::new();
            }
            self.settling_from = None;
        }
        if maximized != Some(true) {
            self.free_height = inner.clamp(FREE_MIN_HEIGHT, MAX_HEIGHT);
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests;
