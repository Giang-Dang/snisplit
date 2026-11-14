use std::io;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use eframe::egui;
use snisplit::sites;
use snisplit_gui::driver::Driver;
use snisplit_gui::state::{Effect, Event, Offer, Probe, ProbeView, Purpose, Site, SiteKind, State};

const TITLE: &str = "snisplit";
const WINDOW_SIZE: [f32; 2] = [820.0, 720.0];
const LOG_HEIGHT: f32 = 110.0;
const LOG_MIN_HEIGHT: f32 = 50.0;
const LOG_MAX_SHARE: f32 = 0.5;
const DOMAIN_FIELD_WIDTH: f32 = 260.0;
const START: &str = "Start";

pub fn run() -> ExitCode {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size(WINDOW_SIZE),
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
    domain_field: String,
    exit_allowed: bool,
}

impl App {
    fn new(ctx: &egui::Context) -> io::Result<Self> {
        let repaint = ctx.clone();
        let (mut driver, events) =
            Driver::new(sites::folder(), Arc::new(move || repaint.request_repaint()))?;
        driver.run(Effect::LoadSites);
        Ok(Self {
            state: State::new(),
            driver,
            events,
            domain_field: String::new(),
            exit_allowed: false,
        })
    }

    fn handle(&mut self, ctx: &egui::Context, event: Event) {
        for effect in self.state.update(event) {
            if effect == Effect::Exit {
                self.exit_allowed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.driver.run(effect);
            }
        }
    }

    fn controls(&self, ui: &mut egui::Ui, pressed: &mut Vec<Event>) {
        ui.horizontal(|ui| {
            match self.state.start_stop_label() {
                Some(label) => {
                    if ui.button(label).clicked() {
                        pressed.push(if label == START {
                            Event::StartPressed
                        } else {
                            Event::StopPressed
                        });
                    }
                }
                None => {
                    ui.add_enabled(false, egui::Button::new("Stop"));
                }
            }
            ui.strong(self.state.status());
        });
        if let Some(failure) = self.state.failure() {
            ui.label(failure);
        }
        if let Some(notice) = &self.state.notice {
            ui.label(notice);
        }
    }

    fn site_list(&self, ui: &mut egui::Ui, pressed: &mut Vec<Event>) {
        ui.heading("Sites");
        for site in &self.state.sites {
            site_row(ui, site, !self.state.probing(), pressed);
        }
    }

    fn add_field(&mut self, ui: &mut egui::Ui, pressed: &mut Vec<Event>) {
        ui.heading("Add a site");
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.domain_field)
                    .hint_text("domain, for example example.com")
                    .desired_width(DOMAIN_FIELD_WIDTH),
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let check = ui.add_enabled(!self.state.probing(), egui::Button::new("Check"));
            if (check.clicked() || entered) && !self.state.probing() {
                pressed.push(Event::ProbePressed(self.domain_field.clone()));
            }
        });
    }

    fn probe_result(&self, ui: &mut egui::Ui, pressed: &mut Vec<Event>) {
        match &self.state.probe {
            Probe::Idle => {}
            Probe::Running { domain, .. } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Checking {domain}, this takes a few seconds..."));
                });
            }
            Probe::Failed { domain, message } => {
                ui.label(format!("Could not check {domain}: {message}"));
            }
            Probe::Done { purpose, view } => probe_view(ui, *purpose, view, pressed),
        }
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

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut pressed = Vec::new();
        egui::Panel::top("controls").show(ui, |ui| {
            ui.add_space(4.0);
            self.controls(ui, &mut pressed);
            ui.add_space(4.0);
        });
        let log_max = ui.available_height() * LOG_MAX_SHARE;
        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(LOG_HEIGHT)
            .size_range(LOG_MIN_HEIGHT..=log_max.max(LOG_MIN_HEIGHT))
            .show(ui, |ui| log_pane(ui, &self.state));
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("main")
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(ui, |ui| {
                    self.site_list(ui, &mut pressed);
                    ui.separator();
                    self.add_field(ui, &mut pressed);
                    self.probe_result(ui, &mut pressed);
                });
        });
        let ctx = ui.ctx().clone();
        for event in pressed {
            self.handle(&ctx, event);
        }
    }
}

fn site_row(ui: &mut egui::Ui, site: &Site, can_check: bool, pressed: &mut Vec<Event>) {
    ui.horizontal(|ui| {
        ui.strong(&site.name);
        let kind = match &site.kind {
            SiteKind::BuiltIn => "built-in",
            SiteKind::User => "your site",
            SiteKind::Broken(_) => "file has a problem",
        };
        ui.label(format!("({kind})"));
        let checkable = can_check && !site.domains.is_empty();
        if ui
            .add_enabled(checkable, egui::Button::new("Check"))
            .clicked()
        {
            pressed.push(Event::CheckPressed(site.name.clone()));
        }
        if site.removable() && ui.button("Remove").clicked() {
            pressed.push(Event::RemovePressed(site.name.clone()));
        }
    });
    let details = match &site.kind {
        SiteKind::Broken(problem) => problem.clone(),
        SiteKind::BuiltIn | SiteKind::User => site.domains.join(", "),
    };
    ui.label(details);
    ui.add_space(4.0);
}

fn probe_view(ui: &mut egui::Ui, purpose: Purpose, view: &ProbeView, pressed: &mut Vec<Event>) {
    ui.add_space(6.0);
    ui.strong(format!("{} at {}", view.domain, view.address));
    let runs = view
        .rows
        .iter()
        .map(|row| row.runs.len())
        .max()
        .unwrap_or(0);
    egui::Grid::new("attempts").striped(true).show(ui, |ui| {
        ui.strong("attempt");
        ui.strong("result");
        for run in 1..=runs {
            ui.strong(format!("run {run}"));
        }
        ui.end_row();
        for row in &view.rows {
            ui.label(&row.attempt);
            ui.label(&row.result);
            for outcome in &row.runs {
                ui.label(outcome);
            }
            ui.end_row();
        }
    });
    ui.label(format!("Verdict: {}", view.summary));
    if purpose != Purpose::Add {
        return;
    }
    match &view.offer {
        Offer::Add(_) => {
            if ui.button(view.offer.label()).clicked() {
                pressed.push(Event::AddPressed);
            }
        }
        Offer::NotBlocked | Offer::NothingWorked => {
            ui.strong(view.offer.label());
        }
    }
}

fn log_pane(ui: &mut egui::Ui, state: &State) {
    ui.strong("Log");
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for line in state.log() {
                ui.monospace(line);
            }
        });
}
