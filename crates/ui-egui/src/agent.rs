//! The Agent panel: a chat with Redrob Code, which drives this window through its MCP tools.
//!
//! The UI stays process-neutral: the agent process and its protocol live behind [`AgentHost`],
//! supplied by the desktop app. Without one (tests) the panel says so.

use egui::{Color32, RichText};

/// One choice in a permission request, as the agent offered it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once` or `reject_always`.
    pub kind: String,
}

/// What the agent process reports, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentEvent {
    Ready {
        name: String,
    },
    Text(String),
    Thought(String),
    Tool {
        id: String,
        title: String,
        status: String,
    },
    /// The agent asks before running a tool; answer with [`AgentHost::answer`].
    Permission {
        request: u64,
        title: String,
        options: Vec<PermissionOption>,
    },
    TurnEnd {
        stop: String,
    },
    Notice(String),
    Error(String),
}

/// A running agent session. Every call returns at once; results arrive through [`Self::poll`].
pub trait AgentHost {
    fn prompt(&mut self, text: &str);
    fn cancel(&mut self);
    /// Answer a [`AgentEvent::Permission`]; `None` declines.
    fn answer(&mut self, request: u64, option: Option<&str>);
    fn poll(&mut self) -> Vec<AgentEvent>;
}

/// Starts an agent session on first use.
pub type AgentFactory = Box<dyn FnMut() -> Result<Box<dyn AgentHost>, String>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    User(String),
    Agent(String),
    Thought(String),
    Tool { id: String, title: String, status: String },
    Notice(String),
    Error(String),
}

#[derive(Default)]
pub struct AgentPanel {
    factory: Option<AgentFactory>,
    host: Option<Box<dyn AgentHost>>,
    pub entries: Vec<Entry>,
    pub input: String,
    pub busy: bool,
    pub agent_name: Option<String>,
    pub pending: Option<(u64, String, Vec<PermissionOption>)>,
    pub show_thoughts: bool,
}

impl AgentPanel {
    pub fn new(factory: Option<AgentFactory>) -> Self {
        Self { factory, ..Default::default() }
    }

    pub fn available(&self) -> bool {
        self.factory.is_some()
    }

    /// Fold one event into the transcript. Text and thought chunks extend the open entry.
    pub fn apply(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::Ready { name } => self.agent_name = Some(name),
            AgentEvent::Text(s) => match self.entries.last_mut() {
                Some(Entry::Agent(t)) => t.push_str(&s),
                _ => self.entries.push(Entry::Agent(s)),
            },
            AgentEvent::Thought(s) => match self.entries.last_mut() {
                Some(Entry::Thought(t)) => t.push_str(&s),
                _ => self.entries.push(Entry::Thought(s)),
            },
            AgentEvent::Tool { id, title, status } => {
                let found = self.entries.iter_mut().rev().find_map(|e| match e {
                    Entry::Tool { id: i, title: t, status: s } if *i == id => Some((t, s)),
                    _ => None,
                });
                match found {
                    Some((t, s)) => {
                        if !title.is_empty() {
                            *t = title;
                        }
                        if !status.is_empty() {
                            *s = status;
                        }
                    }
                    None => self.entries.push(Entry::Tool { id, title, status }),
                }
            }
            AgentEvent::Permission { request, title, options } => self.pending = Some((request, title, options)),
            AgentEvent::TurnEnd { stop } => {
                self.busy = false;
                self.pending = None;
                if stop != "end_turn" {
                    self.entries.push(Entry::Notice(match stop.as_str() {
                        "cancelled" => "Stopped.".to_string(),
                        "max_tokens" => "The reply hit its length limit.".to_string(),
                        "refusal" => "Redrob Code declined this request.".to_string(),
                        other => format!("The turn ended: {other}."),
                    }));
                }
            }
            AgentEvent::Notice(s) => self.entries.push(Entry::Notice(s)),
            AgentEvent::Error(s) => {
                self.busy = false;
                self.pending = None;
                self.entries.push(Entry::Error(s));
            }
        }
    }

    /// Send the typed prompt, starting the agent on first use.
    pub fn send(&mut self) {
        let text = std::mem::take(&mut self.input).trim().to_string();
        if text.is_empty() || self.busy {
            self.input = text;
            return;
        }
        if self.host.is_none() {
            match self.factory.as_mut().map(|f| f()) {
                Some(Ok(h)) => self.host = Some(h),
                Some(Err(e)) => {
                    self.entries.push(Entry::Error(e));
                    return;
                }
                None => {
                    self.entries.push(Entry::Error("No agent is available in this build.".into()));
                    return;
                }
            }
        }
        self.entries.push(Entry::User(text.clone()));
        self.busy = true;
        if let Some(h) = &mut self.host {
            h.prompt(&text);
        }
    }

    pub fn cancel(&mut self) {
        if let Some(h) = &mut self.host {
            h.cancel();
        }
    }

    pub fn answer(&mut self, option: Option<&str>) {
        if let (Some((request, _, _)), Some(h)) = (self.pending.take(), &mut self.host) {
            h.answer(request, option);
        }
    }

    /// Drain the host's events. Returns true when anything changed.
    pub fn poll(&mut self) -> bool {
        let Some(h) = &mut self.host else { return false };
        let events = h.poll();
        let changed = !events.is_empty();
        for ev in events {
            self.apply(ev);
        }
        changed
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if self.poll() {
            ui.ctx().request_repaint();
        }
        ui.horizontal(|ui| {
            ui.heading("Agent");
            if let Some(name) = &self.agent_name {
                ui.label(RichText::new(name).weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut self.show_thoughts, "thoughts");
                if self.busy && ui.small_button("Stop").clicked() {
                    self.cancel();
                }
            });
        });
        ui.separator();
        if !self.available() {
            ui.label(RichText::new("No agent in this build. The desktop app connects Redrob Code here.").weak());
        }
        let avail = ui.available_height() - 80.0;
        egui::ScrollArea::vertical().max_height(avail.max(80.0)).stick_to_bottom(true).show(ui, |ui| {
            for e in &self.entries {
                match e {
                    Entry::User(t) => {
                        ui.label(RichText::new(t).strong());
                    }
                    Entry::Agent(t) => {
                        ui.label(t);
                    }
                    Entry::Thought(t) => {
                        if self.show_thoughts {
                            ui.label(RichText::new(t).weak().italics());
                        }
                    }
                    Entry::Tool { title, status, .. } => {
                        let mark = match status.as_str() {
                            "completed" => "✓",
                            "failed" => "✗",
                            _ => "…",
                        };
                        ui.label(RichText::new(format!("{mark} {title}")).color(Color32::from_rgb(0xaa, 0xb0, 0xbb)));
                    }
                    Entry::Notice(t) => {
                        ui.label(RichText::new(t).weak());
                    }
                    Entry::Error(t) => {
                        ui.label(RichText::new(t).color(Color32::from_rgb(0xe0, 0x60, 0x60)));
                    }
                }
                ui.add_space(4.0);
            }
        });
        if let Some((_, title, options)) = self.pending.clone() {
            ui.separator();
            ui.label(RichText::new(format!("Allow: {title}?")).strong());
            ui.horizontal_wrapped(|ui| {
                for o in &options {
                    if ui.button(&o.name).clicked() {
                        self.answer(Some(&o.id));
                    }
                }
                if ui.button("Decline").clicked() {
                    self.answer(None);
                }
            });
        }
        ui.separator();
        let response = ui.add(
            egui::TextEdit::multiline(&mut self.input).desired_rows(2).hint_text("Ask the agent to model something…").desired_width(f32::INFINITY),
        );
        let enter = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.busy, egui::Button::new("Send")).clicked() || enter {
                if enter {
                    // the Enter that triggered send also inserted a newline
                    self.input = self.input.trim_end().to_string();
                }
                self.send();
            }
            if self.busy {
                ui.spinner();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo(Vec<AgentEvent>);
    impl AgentHost for Echo {
        fn prompt(&mut self, text: &str) {
            self.0.push(AgentEvent::Text(format!("echo: {text}")));
            self.0.push(AgentEvent::TurnEnd { stop: "end_turn".into() });
        }
        fn cancel(&mut self) {}
        fn answer(&mut self, _: u64, _: Option<&str>) {}
        fn poll(&mut self) -> Vec<AgentEvent> {
            std::mem::take(&mut self.0)
        }
    }

    #[test]
    fn send_poll_and_transcript_folding() {
        let factory: AgentFactory = Box::new(|| Ok(Box::new(Echo(Vec::new())) as Box<dyn AgentHost>));
        let mut panel = AgentPanel::new(Some(factory));
        panel.input = "make a cube".into();
        panel.send();
        assert!(panel.busy);
        assert_eq!(panel.entries[0], Entry::User("make a cube".into()));
        assert!(panel.poll());
        assert!(!panel.busy);
        assert_eq!(panel.entries[1], Entry::Agent("echo: make a cube".into()));
        panel.apply(AgentEvent::Text(" more".into()));
        assert_eq!(panel.entries[1], Entry::Agent("echo: make a cube more".into()));
        panel.apply(AgentEvent::Tool { id: "t1".into(), title: "add_primitive".into(), status: "pending".into() });
        panel.apply(AgentEvent::Tool { id: "t1".into(), title: String::new(), status: "completed".into() });
        assert_eq!(panel.entries.last(), Some(&Entry::Tool { id: "t1".into(), title: "add_primitive".into(), status: "completed".into() }));
        panel.apply(AgentEvent::Permission { request: 7, title: "mesh_op".into(), options: vec![] });
        assert!(panel.pending.is_some());
        panel.apply(AgentEvent::TurnEnd { stop: "cancelled".into() });
        assert!(panel.pending.is_none());
        assert_eq!(panel.entries.last(), Some(&Entry::Notice("Stopped.".into())));
    }

    #[test]
    fn without_a_factory_send_reports() {
        let mut panel = AgentPanel::new(None);
        panel.input = "hi".into();
        panel.send();
        assert!(matches!(panel.entries.last(), Some(Entry::Error(_))));
        assert!(!panel.busy);
    }
}
