use std::sync::mpsc::Sender;
use std::thread;

use crate::crypto::{FheType, TfheClient};

// ── Background messages ──────────────────────────────────────────────────────

pub enum BgMsg {
    KeysReady(TfheClient),
    HttpResponse { status: u16, body: String },
    HttpError(String),
}

// ── Data types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub encrypted: bool,
    pub fhe_type: FheType, // only relevant when encrypted == true
}

// ── App modes ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Loading,
    Normal,
    EditUrl,
    EditVar,
    Response,
    ServerKey, // show the server key for copying
}

#[derive(Debug, Clone, PartialEq)]
pub enum VarField {
    Name,
    Value,
    Encrypted,
    FheType,
}

// ── Popup state for add/edit variable ────────────────────────────────────────

pub struct EditState {
    pub name: String,
    pub value: String,
    pub encrypted: bool,
    pub fhe_type_idx: usize,
    pub field: VarField,
    pub editing_idx: Option<usize>,
}

impl Default for EditState {
    fn default() -> Self {
        EditState {
            name: String::new(),
            value: String::new(),
            encrypted: false,
            fhe_type_idx: 3, // default: UInt64
            field: VarField::Name,
            editing_idx: None,
        }
    }
}

impl EditState {
    pub fn selected_fhe_type(&self) -> &FheType {
        &FheType::all()[self.fhe_type_idx]
    }
}

// ── Main app state ────────────────────────────────────────────────────────────

pub struct App {
    pub mode: Mode,
    // URL bar
    pub url: String,
    pub url_cursor: usize,
    // HTTP method
    pub method: &'static str,
    pub method_idx: usize,
    // Variables
    pub variables: Vec<Variable>,
    pub var_selected: usize,
    // TFHE
    pub tfhe: Option<TfheClient>,
    pub server_key_b64: Option<String>,
    // Response
    pub response_status: Option<u16>,
    pub response_body: String,
    pub response_scroll: u16,
    pub response_fields: Vec<(String, String)>,
    pub decrypt_selected: usize,
    pub decrypt_result: Option<String>,
    pub decrypt_type_idx: usize,
    // Status
    pub status_msg: String,
    pub sending: bool,
    // Add/edit popup
    pub edit: EditState,
    // Background channel sender
    bg_tx: Sender<BgMsg>,
}

const METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH"];

impl App {
    pub fn new(bg_tx: Sender<BgMsg>) -> Self {
        let url = "http://localhost:3000/".to_string();
        let url_cursor = url.len();
        App {
            mode: Mode::Loading,
            url,
            url_cursor,
            method: METHODS[1],
            method_idx: 1,
            variables: Vec::new(),
            var_selected: 0,
            tfhe: None,
            server_key_b64: None,
            response_status: None,
            response_body: String::new(),
            response_scroll: 0,
            response_fields: Vec::new(),
            decrypt_selected: 0,
            decrypt_result: None,
            decrypt_type_idx: 3, // UInt64
            status_msg: "Generating TFHE keys, please wait...".to_string(),
            sending: false,
            edit: EditState::default(),
            bg_tx,
        }
    }

    pub fn start_key_generation(&self) {
        let tx = self.bg_tx.clone();
        thread::spawn(move || {
            let client = TfheClient::generate();
            let _ = tx.send(BgMsg::KeysReady(client));
        });
    }

    pub fn handle_bg(&mut self, msg: BgMsg) {
        match msg {
            BgMsg::KeysReady(client) => {
                // Pre-compute the server key export
                self.server_key_b64 = client.server_key_b64().ok();
                self.tfhe = Some(client);
                self.mode = Mode::Normal;
                self.status_msg =
                    "Ready. Press [k] to view server key (send to your server).".to_string();
            }
            BgMsg::HttpResponse { status, body } => {
                self.sending = false;
                self.response_status = Some(status);
                self.response_body = body.clone();
                self.response_scroll = 0;
                self.response_fields = parse_json_fields(&body);
                self.decrypt_selected = 0;
                self.decrypt_result = None;
                self.mode = Mode::Response;
                self.status_msg = format!("Response received: HTTP {}", status);
            }
            BgMsg::HttpError(err) => {
                self.sending = false;
                self.status_msg = format!("Request failed: {}", err);
            }
        }
    }

    // ── Key handler entry point ───────────────────────────────────────────────

    pub fn on_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }

        let mode = self.mode.clone();
        match mode {
            Mode::Loading => {
                if key.code == KeyCode::Char('q') {
                    return true;
                }
            }
            Mode::Normal => return self.handle_normal_key(key),
            Mode::EditUrl => self.handle_url_key(key),
            Mode::EditVar => self.handle_editvar_key(key),
            Mode::Response => self.handle_response_key(key),
            Mode::ServerKey => {
                // any key closes the server key view
                self.mode = Mode::Normal;
            }
        }
        false
    }

    // ── Normal mode ───────────────────────────────────────────────────────────

    fn handle_normal_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char('u') => self.mode = Mode::EditUrl,
            KeyCode::Char('m') => {
                self.method_idx = (self.method_idx + 1) % METHODS.len();
                self.method = METHODS[self.method_idx];
            }
            KeyCode::Char('a') => {
                self.edit = EditState::default();
                self.mode = Mode::EditVar;
            }
            KeyCode::Char('e') => {
                if !self.variables.is_empty() {
                    let v = &self.variables[self.var_selected];
                    let fhe_type_idx = FheType::all()
                        .iter()
                        .position(|t| t == &v.fhe_type)
                        .unwrap_or(3);
                    self.edit = EditState {
                        name: v.name.clone(),
                        value: v.value.clone(),
                        encrypted: v.encrypted,
                        fhe_type_idx,
                        field: VarField::Name,
                        editing_idx: Some(self.var_selected),
                    };
                    self.mode = Mode::EditVar;
                }
            }
            KeyCode::Char('d') => {
                if !self.variables.is_empty() {
                    self.variables.remove(self.var_selected);
                    if self.var_selected > 0 && self.var_selected >= self.variables.len() {
                        self.var_selected = self.variables.len().saturating_sub(1);
                    }
                    self.status_msg = "Variable deleted.".to_string();
                }
            }
            KeyCode::Char('s') | KeyCode::F(5) => self.send_request(),
            KeyCode::Char('r') => {
                if !self.response_body.is_empty() {
                    self.mode = Mode::Response;
                }
            }
            KeyCode::Char('k') => {
                if self.server_key_b64.is_some() {
                    self.mode = Mode::ServerKey;
                } else {
                    self.status_msg = "Server key not ready yet.".to_string();
                }
            }
            KeyCode::Up => {
                if self.var_selected > 0 {
                    self.var_selected -= 1;
                }
            }
            KeyCode::Down => {
                if !self.variables.is_empty() && self.var_selected + 1 < self.variables.len() {
                    self.var_selected += 1;
                }
            }
            _ => {}
        }
        false
    }

    // ── URL editing mode ──────────────────────────────────────────────────────

    fn handle_url_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Enter | KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Char(c) => {
                self.url.insert(self.url_cursor, c);
                self.url_cursor += c.len_utf8();
            }
            KeyCode::Backspace => {
                if self.url_cursor > 0 {
                    let before = &self.url[..self.url_cursor];
                    if let Some((idx, ch)) = before.char_indices().last() {
                        self.url.remove(idx);
                        self.url_cursor -= ch.len_utf8();
                    }
                }
            }
            KeyCode::Delete => {
                if self.url_cursor < self.url.len() {
                    self.url.remove(self.url_cursor);
                }
            }
            KeyCode::Left => {
                let before = &self.url[..self.url_cursor];
                if let Some((idx, _)) = before.char_indices().last() {
                    self.url_cursor = idx;
                }
            }
            KeyCode::Right => {
                if self.url_cursor < self.url.len() {
                    let ch = self.url[self.url_cursor..].chars().next().unwrap();
                    self.url_cursor += ch.len_utf8();
                }
            }
            KeyCode::Home => self.url_cursor = 0,
            KeyCode::End => self.url_cursor = self.url.len(),
            _ => {}
        }
    }

    // ── Variable add/edit mode ────────────────────────────────────────────────

    fn handle_editvar_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                match self.edit.field {
                    VarField::Encrypted => self.edit.encrypted = !self.edit.encrypted,
                    VarField::FheType => { /* Enter on type row does nothing extra */ }
                    _ => self.save_variable(),
                }
            }
            KeyCode::Tab | KeyCode::Down => {
                self.edit.field = match &self.edit.field {
                    VarField::Name => VarField::Value,
                    VarField::Value => VarField::Encrypted,
                    VarField::Encrypted => {
                        if self.edit.encrypted {
                            VarField::FheType
                        } else {
                            VarField::Name
                        }
                    }
                    VarField::FheType => VarField::Name,
                };
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.edit.field = match &self.edit.field {
                    VarField::Name => {
                        if self.edit.encrypted {
                            VarField::FheType
                        } else {
                            VarField::Encrypted
                        }
                    }
                    VarField::Value => VarField::Name,
                    VarField::Encrypted => VarField::Value,
                    VarField::FheType => VarField::Encrypted,
                };
            }
            KeyCode::Left => {
                if self.edit.field == VarField::FheType && self.edit.fhe_type_idx > 0 {
                    self.edit.fhe_type_idx -= 1;
                }
            }
            KeyCode::Right => {
                if self.edit.field == VarField::FheType
                    && self.edit.fhe_type_idx + 1 < FheType::all().len()
                {
                    self.edit.fhe_type_idx += 1;
                }
            }
            KeyCode::Char(' ') => {
                if self.edit.field == VarField::Encrypted {
                    self.edit.encrypted = !self.edit.encrypted;
                    // reset to Encrypted field if type was selected but now plain
                    if !self.edit.encrypted && self.edit.field == VarField::FheType {
                        self.edit.field = VarField::Encrypted;
                    }
                } else {
                    self.push_edit_char(' ');
                }
            }
            KeyCode::Char(c) => self.push_edit_char(c),
            KeyCode::Backspace => match self.edit.field {
                VarField::Name => {
                    self.edit.name.pop();
                }
                VarField::Value => {
                    self.edit.value.pop();
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn push_edit_char(&mut self, c: char) {
        match self.edit.field {
            VarField::Name => self.edit.name.push(c),
            VarField::Value => self.edit.value.push(c),
            _ => {}
        }
    }

    fn save_variable(&mut self) {
        if self.edit.name.trim().is_empty() {
            self.status_msg = "Name cannot be empty.".to_string();
            return;
        }
        // Validate value against selected type
        if self.edit.encrypted {
            let fhe_type = self.edit.selected_fhe_type().clone();
            if !fhe_type.supports_value(&self.edit.value) {
                self.status_msg = format!(
                    "Value '{}' is not valid for type {}",
                    self.edit.value,
                    fhe_type.label()
                );
                return;
            }
        }
        let var = Variable {
            name: self.edit.name.trim().to_string(),
            value: self.edit.value.clone(),
            encrypted: self.edit.encrypted,
            fhe_type: self.edit.selected_fhe_type().clone(),
        };
        match self.edit.editing_idx {
            Some(idx) => {
                self.variables[idx] = var;
                self.status_msg = "Variable updated.".to_string();
            }
            None => {
                self.variables.push(var);
                self.var_selected = self.variables.len() - 1;
                self.status_msg = "Variable added.".to_string();
            }
        }
        self.mode = Mode::Normal;
    }

    // ── Response view mode ────────────────────────────────────────────────────

    fn handle_response_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                self.mode = Mode::Normal;
                self.decrypt_result = None;
            }
            KeyCode::Up => self.response_scroll = self.response_scroll.saturating_sub(1),
            KeyCode::Down => self.response_scroll = self.response_scroll.saturating_add(1),
            KeyCode::PageUp => self.response_scroll = self.response_scroll.saturating_sub(10),
            KeyCode::PageDown => self.response_scroll = self.response_scroll.saturating_add(10),
            // Navigate fields
            KeyCode::Left => {
                if self.decrypt_selected > 0 {
                    self.decrypt_selected -= 1;
                    self.decrypt_result = None;
                }
            }
            KeyCode::Right => {
                if !self.response_fields.is_empty()
                    && self.decrypt_selected + 1 < self.response_fields.len()
                {
                    self.decrypt_selected += 1;
                    self.decrypt_result = None;
                }
            }
            // Select decryption type
            KeyCode::Char('[') => {
                if self.decrypt_type_idx > 0 {
                    self.decrypt_type_idx -= 1;
                    self.decrypt_result = None;
                }
            }
            KeyCode::Char(']') => {
                if self.decrypt_type_idx + 1 < FheType::all().len() {
                    self.decrypt_type_idx += 1;
                    self.decrypt_result = None;
                }
            }
            // Decrypt
            KeyCode::Char('d') => {
                if let Some(tfhe) = &self.tfhe {
                    if let Some((_name, value)) = self.response_fields.get(self.decrypt_selected) {
                        let fhe_type = &FheType::all()[self.decrypt_type_idx];
                        self.decrypt_result = Some(match tfhe.decrypt(value, fhe_type) {
                            Ok(v) => v,
                            Err(e) => format!("Error: {}", e),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    // ── HTTP request dispatch ─────────────────────────────────────────────────

    pub fn send_request(&mut self) {
        if self.url.trim().is_empty() {
            self.status_msg = "URL is empty.".to_string();
            return;
        }
        let Some(tfhe) = &self.tfhe else {
            self.status_msg = "TFHE keys not ready yet.".to_string();
            return;
        };

        let mut body_map = serde_json::Map::new();
        for var in &self.variables {
            let json_val = if var.encrypted {
                match tfhe.encrypt(&var.value, &var.fhe_type) {
                    Ok(b64) => serde_json::Value::String(b64),
                    Err(e) => {
                        self.status_msg = format!("Encrypt '{}': {}", var.name, e);
                        return;
                    }
                }
            } else if let Ok(n) = var.value.parse::<i64>() {
                serde_json::Value::Number(n.into())
            } else {
                serde_json::Value::String(var.value.clone())
            };
            body_map.insert(var.name.clone(), json_val);
        }

        self.sending = true;
        self.status_msg = "Sending request...".to_string();

        let url = self.url.clone();
        let method = self.method.to_string();
        let body = serde_json::Value::Object(body_map);
        let tx = self.bg_tx.clone();

        thread::spawn(move || {
            match send_http(&url, &method, body) {
                Ok((status, resp_body)) => {
                    let _ = tx.send(BgMsg::HttpResponse {
                        status,
                        body: resp_body,
                    });
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::HttpError(e.to_string()));
                }
            }
        });
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn parse_json_fields(body: &str) -> Vec<(String, String)> {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(obj) = val.as_object() else {
        return Vec::new();
    };
    obj.iter()
        .map(|(k, v)| {
            let s = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.clone(), s)
        })
        .collect()
}

fn send_http(
    url: &str,
    method: &str,
    body: serde_json::Value,
) -> anyhow::Result<(u16, String)> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = match method {
        "GET" => {
            let mut builder = client.get(url);
            if let serde_json::Value::Object(ref map) = body {
                for (k, v) in map {
                    let s = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    builder = builder.query(&[(k.as_str(), s)]);
                }
            }
            builder.send()?
        }
        "DELETE" => client.delete(url).send()?,
        "PUT" => client.put(url).json(&body).send()?,
        "PATCH" => client.patch(url).json(&body).send()?,
        _ => client.post(url).json(&body).send()?,
    };

    let status = resp.status().as_u16();
    let text = resp.text()?;
    Ok((status, text))
}
