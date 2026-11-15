use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, CornerRadius, FontId, Frame, Key, Label, Layout, Margin, RichText, ScrollArea,
    Sense, Stroke, TextEdit, Theme, Ui, Vec2, vec2,
};
use snisplit::sites;
use snisplit_gui::driver::{Driver, Folders};
use snisplit_gui::state::{Effect, Event, Page, Probing, Row, State};
use snisplit_gui::view::{self, AddPanel, Button, Details, Hero, Ring, SiteRow, Style, Tone};

use crate::widgets::{
    self, ACCENT, BACKGROUND, BORDER, DEEP, HERO_ON, HERO_TROUBLE, LIGHT, LINE, MUTED, ON_ACCENT,
    ON_LIGHT, PALE, ROW, SOFT, TEXT, TRACK, TROUBLE_TEXT,
};

const TITLE: &str = "snisplit";
const WIDTH: f32 = 400.0;
const HEIGHT: f32 = 680.0;
const MIN_HEIGHT: f32 = 480.0;
const MAX_HEIGHT: f32 = 1600.0;
const FULL_HERO_HEIGHT: f32 = 300.0;
const FOOTER_HEIGHT: f32 = 38.0;
const LIST_HEADING_HEIGHT: f32 = 44.0;
const FULL_ROW_HEIGHT: f32 = 58.0;
const ADD_ROW_HEIGHT: f32 = 46.0;
const BIG_BUTTON: f32 = 108.0;
const SMALL_BUTTON: f32 = 56.0;
const BIG_LABEL: f32 = 19.0;
const SMALL_LABEL: f32 = 14.0;
const SWITCH_AREA: f32 = 104.0;
const SAVE_ROW_HEIGHT: f32 = 48.0;
const BUTTON_PADDING: f32 = 12.0;
const BUTTON_GAP: f32 = 8.0;
const DETAILS_INDENT: f32 = 16.0;
const CLOCK_TICK: Duration = Duration::from_secs(15);
const FIELD_HINT: &str = "example.net or a pasted link";

pub fn run() -> ExitCode {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([WIDTH, HEIGHT])
            .with_min_inner_size([WIDTH, MIN_HEIGHT])
            .with_max_inner_size([WIDTH, MAX_HEIGHT])
            .with_resizable(true),
        ..Default::default()
    };
    let outcome = eframe::run_native(
        TITLE,
        options,
        Box::new(|creation| {
            let app = App::new(&creation.egui_ctx)?;
            Ok(Box::new(app))
        }),
    );
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

struct App {
    state: State,
    driver: Driver,
    events: Receiver<Event>,
    exit_allowed: bool,
    reveal_panel: bool,
}

impl App {
    fn new(ctx: &egui::Context) -> io::Result<Self> {
        style(ctx);
        let repaint = ctx.clone();
        let folders = Folders {
            sites: sites::folder(),
            home: home_folder(),
        };
        let (mut driver, events) =
            Driver::new(folders, Arc::new(move || repaint.request_repaint()))?;
        driver.run(Effect::LoadSites);
        Ok(Self {
            state: State::new(),
            driver,
            events,
            exit_allowed: false,
            reveal_panel: false,
        })
    }

    fn handle(&mut self, ctx: &egui::Context, event: Event) {
        if event == Event::AddRowPressed {
            self.reveal_panel = true;
        }
        for effect in self.state.update(event) {
            if effect == Effect::Exit {
                self.exit_allowed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.driver.run(effect);
            }
        }
    }

    fn footer(&self, ui: &mut Ui, compact: bool, now: Instant, pressed: &mut Vec<Event>) {
        ui.horizontal_centered(|ui| {
            match self.state.page() {
                Page::Sites => {
                    let activity = Button {
                        label: "Activity".to_string(),
                        press: Event::ActivityPressed,
                        style: Style::Text,
                        enabled: true,
                    };
                    styled_button(ui, &activity, pressed);
                }
                Page::Activity => {
                    small_text(ui, &view::lines_count(&self.state));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.state.page() == Page::Sites && compact {
                    let add = Button {
                        label: "+ Add a site".to_string(),
                        press: Event::AddRowPressed,
                        style: Style::Text,
                        enabled: true,
                    };
                    styled_button(ui, &add, pressed);
                } else {
                    small_text(ui, &view::footer_note(&self.state, now));
                }
            });
        });
    }

    fn site_list(&mut self, ui: &mut Ui, compact: bool, now: Instant, pressed: &mut Vec<Event>) {
        ui.add_space(2.0);
        ui.label(RichText::new("Sites").size(13.0).color(MUTED));
        ui.add_space(2.0);
        let rows = view::rows(&self.state);
        let details = view::details(&self.state, now);
        let panel = view::add_panel(&self.state, now);
        let probe = self.state.probe().cloned();
        let reveal = std::mem::take(&mut self.reveal_panel);
        ScrollArea::vertical()
            .id_salt("sites")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in &rows {
                    let opened = details.as_ref().filter(|_| row.open);
                    site_row(ui, row, opened, compact, pressed);
                    ui.add_space(6.0);
                }
                match &panel {
                    Some(panel) => add_panel(ui, panel, probe.as_ref(), reveal, now, pressed),
                    None if !compact => add_row(ui, pressed),
                    None => {}
                }
            });
    }

    fn activity(&self, ui: &mut Ui, pressed: &mut Vec<Event>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Activity").size(13.0).color(MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let back = Button {
                    label: "Back to sites".to_string(),
                    press: Event::BackPressed,
                    style: Style::Plain,
                    enabled: true,
                };
                styled_button(ui, &back, pressed);
            });
        });
        ui.add_space(6.0);
        let height = (ui.available_height() - SAVE_ROW_HEIGHT).max(SAVE_ROW_HEIGHT);
        Frame::new()
            .fill(DEEP)
            .corner_radius(8.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("activity")
                    .max_height(height)
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in self.state.log() {
                            ui.add(
                                Label::new(RichText::new(line).monospace().size(12.0).color(PALE))
                                    .wrap(),
                            );
                        }
                    });
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let save = Button {
                label: "Save to a file".to_string(),
                press: Event::SaveLogPressed,
                style: Style::Plain,
                enabled: true,
            };
            styled_button(ui, &save, pressed);
            ui.add(
                Label::new(
                    RichText::new(view::save_note(&self.state))
                        .size(12.0)
                        .color(SOFT),
                )
                .wrap(),
            );
        });
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.events.try_recv() {
            self.handle(ctx, event);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.exit_allowed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.handle(ctx, Event::CloseRequested);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let mut pressed = Vec::new();
        let hero = view::hero(&self.state);
        let compact = self.state.page() == Page::Activity
            || list_overflows(ui.available_height(), self.state.sites().len());
        egui::Panel::top("hero")
            .frame(hero_frame(hero.tone, compact))
            .show(ui, |ui| {
                if compact {
                    compact_hero(ui, &hero, &mut pressed);
                } else {
                    full_hero(ui, &hero, &mut pressed);
                }
            });
        egui::Panel::bottom("footer")
            .exact_size(FOOTER_HEIGHT)
            .frame(
                Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(Margin::symmetric(10, 0)),
            )
            .show(ui, |ui| self.footer(ui, compact, now, &mut pressed));
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(Margin::symmetric(14, 12)),
            )
            .show(ui, |ui| match self.state.page() {
                Page::Sites => self.site_list(ui, compact, now, &mut pressed),
                Page::Activity => self.activity(ui, &mut pressed),
            });
        let ctx = ui.ctx().clone();
        if hero.ring == Ring::Turning || self.state.probe().is_some() {
            ctx.request_repaint();
        } else if self.state.on_since().is_some() {
            ctx.request_repaint_after(CLOCK_TICK);
        }
        for event in pressed {
            self.handle(&ctx, event);
        }
    }
}

fn style(ctx: &egui::Context) {
    ctx.set_theme(Theme::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = BACKGROUND;
    visuals.extreme_bg_color = DEEP;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.5);
    ctx.set_visuals_of(Theme::Dark, visuals);
    ctx.all_styles_mut(|style| {
        style.spacing.button_padding = vec2(BUTTON_PADDING, 4.0);
        style.spacing.item_spacing = vec2(BUTTON_GAP, 6.0);
    });
}

fn home_folder() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

fn list_overflows(window_height: f32, sites: usize) -> bool {
    let room = window_height - FULL_HERO_HEIGHT - FOOTER_HEIGHT;
    let needed = LIST_HEADING_HEIGHT + sites as f32 * FULL_ROW_HEIGHT + ADD_ROW_HEIGHT;
    needed > room
}

fn hero_frame(tone: Tone, compact: bool) -> Frame {
    let fill = match tone {
        Tone::Plain => BACKGROUND,
        Tone::On => HERO_ON,
        Tone::Trouble => HERO_TROUBLE,
    };
    let margin = if compact {
        Margin::symmetric(16, 12)
    } else {
        Margin {
            left: 24,
            right: 24,
            top: 24,
            bottom: 20,
        }
    };
    Frame::new().fill(fill).inner_margin(margin)
}

fn sentence_colour(tone: Tone) -> egui::Color32 {
    match tone {
        Tone::Trouble => TROUBLE_TEXT,
        Tone::Plain | Tone::On => SOFT,
    }
}

fn press_hero(hero: &Hero, clicked: bool, pressed: &mut Vec<Event>) {
    if clicked && let Some(press) = &hero.press {
        pressed.push(press.clone());
    }
}

fn full_hero(ui: &mut Ui, hero: &Hero, pressed: &mut Vec<Event>) {
    ui.vertical_centered(|ui| {
        let clicked = widgets::round_button(
            ui,
            BIG_BUTTON,
            hero.ring,
            hero.action,
            hero.press.is_some(),
            BIG_LABEL,
        )
        .clicked();
        press_hero(hero, clicked, pressed);
        ui.add_space(8.0);
        ui.label(RichText::new(hero.state_line).size(24.0).color(TEXT));
        ui.add_space(4.0);
        for sentence in &hero.sentences {
            ui.add(
                Label::new(
                    RichText::new(sentence)
                        .size(13.0)
                        .color(sentence_colour(hero.tone)),
                )
                .wrap(),
            );
        }
        if !hero.buttons.is_empty() {
            ui.add_space(6.0);
            centered_buttons(ui, &hero.buttons, pressed);
        }
    });
}

fn compact_hero(ui: &mut Ui, hero: &Hero, pressed: &mut Vec<Event>) {
    ui.horizontal(|ui| {
        let clicked = widgets::round_button(
            ui,
            SMALL_BUTTON,
            hero.ring,
            hero.action,
            hero.press.is_some(),
            SMALL_LABEL,
        )
        .clicked();
        press_hero(hero, clicked, pressed);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(hero.state_line).size(18.0).color(TEXT));
            if !hero.compact.is_empty() {
                ui.add(
                    Label::new(
                        RichText::new(&hero.compact)
                            .size(13.0)
                            .color(sentence_colour(hero.tone)),
                    )
                    .wrap(),
                );
            }
            if !hero.buttons.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for button in &hero.buttons {
                        styled_button(ui, button, pressed);
                    }
                });
            }
        });
    });
}

fn centered_buttons(ui: &mut Ui, buttons: &[Button], pressed: &mut Vec<Event>) {
    let font = FontId::proportional(13.0);
    let widths: f32 = buttons
        .iter()
        .map(|button| {
            ui.painter()
                .layout_no_wrap(button.label.clone(), font.clone(), TEXT)
                .size()
                .x
                + 2.0 * BUTTON_PADDING
        })
        .sum();
    let total = widths + BUTTON_GAP * buttons.len().saturating_sub(1) as f32;
    let available = ui.available_width();
    ui.horizontal(|ui| {
        ui.add_space(((available - total) / 2.0).max(0.0));
        for button in buttons {
            styled_button(ui, button, pressed);
        }
    });
}

fn styled_button(ui: &mut Ui, button: &Button, pressed: &mut Vec<Event>) {
    let (fill, text, stroke) = match button.style {
        Style::Primary => (ACCENT, ON_ACCENT, Stroke::new(1.0, ACCENT)),
        Style::Plain => (egui::Color32::TRANSPARENT, TEXT, Stroke::new(1.0, BORDER)),
        Style::Light => (LIGHT, ON_LIGHT, Stroke::new(1.0, LIGHT)),
        Style::Text => (egui::Color32::TRANSPARENT, SOFT, Stroke::NONE),
    };
    let widget = egui::Button::new(RichText::new(&button.label).size(13.0).color(text))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(6))
        .min_size(vec2(0.0, 30.0))
        .frame(button.style != Style::Text);
    if ui.add_enabled(button.enabled, widget).clicked() {
        pressed.push(button.press.clone());
    }
}

fn button_row(ui: &mut Ui, buttons: &[Button], pressed: &mut Vec<Event>) {
    if buttons.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for button in buttons {
            styled_button(ui, button, pressed);
        }
    });
}

fn small_text(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(SOFT));
}

fn wrapped(ui: &mut Ui, text: &str, size: f32, colour: egui::Color32) {
    ui.add(Label::new(RichText::new(text).size(size).color(colour)).wrap());
}

fn site_row(
    ui: &mut Ui,
    row: &SiteRow,
    details: Option<&Details>,
    compact: bool,
    pressed: &mut Vec<Event>,
) {
    let vertical = if compact { 7 } else { 9 };
    Frame::new()
        .fill(ROW)
        .corner_radius(8.0)
        .inner_margin(Margin::symmetric(10, vertical))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if widgets::disclosure(ui, row.open).clicked() {
                    pressed.push(Event::RowPressed(row.name.clone()));
                }
                let name_width = (ui.available_width() - SWITCH_AREA).max(40.0);
                let height = if compact { 22.0 } else { 40.0 };
                ui.allocate_ui_with_layout(
                    vec2(name_width, height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        let colour = if row.dimmed { MUTED } else { TEXT };
                        let name = ui.add(
                            Label::new(RichText::new(&row.title).size(16.0).color(colour))
                                .truncate()
                                .sense(Sense::click()),
                        );
                        if name.clicked() {
                            pressed.push(Event::RowPressed(row.name.clone()));
                        }
                        if !compact {
                            ui.add(
                                Label::new(RichText::new(&row.sub).size(12.0).color(SOFT))
                                    .truncate(),
                            );
                        }
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(switch) = &row.switch {
                        let toggled =
                            widgets::switch(ui, switch.on, switch.busy, switch.enabled).clicked();
                        if toggled {
                            pressed.push(Event::SwitchPressed(row.name.clone()));
                        }
                        ui.label(RichText::new(switch.label).size(12.0).color(SOFT));
                    }
                });
            });
            if let Some(details) = details {
                ui.horizontal(|ui| {
                    ui.add_space(DETAILS_INDENT);
                    ui.vertical(|ui| site_details(ui, details, pressed));
                });
            }
        });
}

fn site_details(ui: &mut Ui, details: &Details, pressed: &mut Vec<Event>) {
    ui.add_space(4.0);
    if !details.domains.is_empty() {
        wrapped(ui, &details.domains, 12.0, SOFT);
    }
    if let Some(sentence) = &details.sentence {
        wrapped(ui, sentence, 13.0, TEXT);
    }
    if let Some(testing) = &details.testing {
        testing_line(ui, testing);
    }
    if let Some(confirm) = &details.confirm {
        Frame::new()
            .fill(ROW)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(8.0)
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.label(RichText::new(&confirm.question).size(14.0).color(TEXT));
                if let Some(note) = confirm.note {
                    wrapped(ui, note, 12.0, SOFT);
                }
                button_row(ui, &confirm.buttons, pressed);
            });
    }
    button_row(ui, &details.buttons, pressed);
    attempts(ui, &details.attempts);
}

fn testing_line(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        widgets::small_ring(ui, 16.0);
        wrapped(ui, text, 14.0, TEXT);
    });
}

fn attempts(ui: &mut Ui, rows: &[Row]) {
    if rows.is_empty() {
        return;
    }
    egui::Grid::new(ui.next_auto_id())
        .num_columns(2)
        .spacing(vec2(10.0, 2.0))
        .show(ui, |ui| {
            for row in rows {
                ui.label(
                    RichText::new(&row.attempt)
                        .monospace()
                        .size(12.0)
                        .color(SOFT),
                );
                ui.label(
                    RichText::new(row.outcomes.join(", "))
                        .monospace()
                        .size(12.0)
                        .color(SOFT),
                );
                ui.end_row();
            }
        });
}

fn add_row(ui: &mut Ui, pressed: &mut Vec<Event>) {
    let response = Frame::new()
        .stroke(Stroke::new(1.0, TRACK))
        .corner_radius(8.0)
        .inner_margin(Margin::symmetric(10, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                widgets::plus(ui.painter(), rect.center(), ACCENT);
                ui.label(RichText::new("Add a site").size(15.0).color(TEXT));
            });
        })
        .response
        .interact(Sense::click());
    if response.clicked() {
        pressed.push(Event::AddRowPressed);
    }
}

fn add_panel(
    ui: &mut Ui,
    panel: &AddPanel,
    probe: Option<&Probing>,
    reveal: bool,
    now: Instant,
    pressed: &mut Vec<Event>,
) {
    let response = Frame::new()
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(8.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Add a site").size(15.0).color(TEXT));
            ui.add_space(4.0);
            let shows_field = panel.field_enabled || panel.testing.is_some();
            if shows_field {
                let mut text = panel.field.clone();
                let field = ui.add(
                    TextEdit::singleline(&mut text)
                        .hint_text(FIELD_HINT)
                        .desired_width(ui.available_width())
                        .font(FontId::proportional(14.0))
                        .text_color(if panel.field_enabled { TEXT } else { MUTED })
                        .interactive(panel.field_enabled),
                );
                if reveal {
                    field.request_focus();
                }
                if field.changed() {
                    pressed.push(Event::FieldEdited(text));
                }
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if entered && panel.field_enabled && probe.is_none() {
                    pressed.push(Event::AddTestPressed { at: now });
                }
            }
            if let Some(hint) = panel.hint {
                wrapped(ui, hint, 12.0, SOFT);
            }
            if let Some(testing) = &panel.testing {
                testing_line(ui, testing);
            }
            if let Some(headline) = &panel.headline {
                ui.horizontal(|ui| {
                    if let Some(mark) = panel.mark {
                        widgets::mark(ui, mark);
                    }
                    wrapped(ui, headline, 16.0, TEXT);
                });
            }
            if let Some(sub) = panel.sub {
                wrapped(ui, sub, 12.0, SOFT);
            }
            button_row(ui, &panel.buttons, pressed);
            attempts(ui, &panel.attempts);
        })
        .response;
    if reveal {
        response.scroll_to_me(Some(Align::Max));
    }
}
