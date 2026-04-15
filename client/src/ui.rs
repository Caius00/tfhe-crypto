use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, Mode, VarField};
use crate::crypto::FheType;

// ── Main entry point ──────────────────────────────────────────────────────────

pub fn draw(f: &mut Frame, app: &App) {
    match app.mode {
        Mode::Loading => draw_loading(f, app),
        Mode::Normal | Mode::EditUrl => draw_main(f, app),
        Mode::EditVar => {
            draw_main(f, app);
            draw_editvar_popup(f, app);
        }
        Mode::Response => draw_response(f, app),
        Mode::ServerKey => draw_server_key(f, app),
    }
}

// ── Loading screen ────────────────────────────────────────────────────────────

fn draw_loading(f: &mut Frame, app: &App) {
    let area = f.area();
    let outer = Block::default()
        .title(" TFHE Postman ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Min(8),
            Constraint::Percentage(35),
        ])
        .split(inner);

    let text = vec![
        Line::from(Span::styled(
            " TFHE Postman ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Generating TFHE keys...",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(""),
        Line::from(Span::styled(
            &app.status_msg,
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Key generation takes ~5-15 seconds.",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let para = Paragraph::new(text).alignment(Alignment::Center);
    f.render_widget(para, vchunks[1]);
}

// ── Main screen ───────────────────────────────────────────────────────────────

fn draw_main(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // URL bar
            Constraint::Min(6),    // Variable list
            Constraint::Length(6), // Response preview
            Constraint::Length(1), // Status
            Constraint::Length(1), // Help
        ])
        .split(area);

    draw_url_bar(f, app, chunks[0]);
    draw_variables(f, app, chunks[1]);
    draw_response_preview(f, app, chunks[2]);
    draw_status(f, app, chunks[3]);
    draw_help_main(f, app, chunks[4]);
}

fn draw_url_bar(f: &mut Frame, app: &App, area: Rect) {
    let editing = app.mode == Mode::EditUrl;
    let block = Block::default()
        .title(" URL ")
        .borders(Borders::ALL)
        .border_style(if editing {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        });

    let method_span = Span::styled(
        format!(" {} ", app.method),
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let url_line = if editing {
        let cursor = app.url_cursor.min(app.url.len());
        let before = &app.url[..cursor];
        let rest = &app.url[cursor..];
        let (at, after) = if let Some(c) = rest.chars().next() {
            (c.to_string(), &rest[c.len_utf8()..])
        } else {
            (" ".to_string(), "")
        };
        Line::from(vec![
            method_span,
            Span::raw("  "),
            Span::styled(before, Style::default().fg(Color::Yellow)),
            Span::styled(at, Style::default().fg(Color::Black).bg(Color::Yellow)),
            Span::styled(after, Style::default().fg(Color::Yellow)),
        ])
    } else {
        Line::from(vec![
            method_span,
            Span::raw("  "),
            Span::styled(app.url.as_str(), Style::default().fg(Color::White)),
        ])
    };

    f.render_widget(Paragraph::new(url_line).block(block), area);
}

fn draw_variables(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Body Variables ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let header = Line::from(Span::styled(
        format!("  {:<22} {:<22} {:<10} {}", "Name", "Value", "Enc", "Type"),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::UNDERLINED),
    ));

    let mut lines: Vec<Line> = vec![header];

    if app.variables.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No variables. Press [a] to add.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, var) in app.variables.iter().enumerate() {
            let selected = i == app.var_selected;
            let bg = if selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            let prefix = if selected { "> " } else { "  " };
            let (enc_label, enc_style) = if var.encrypted {
                ("[TFHE]", Style::default().fg(Color::Green))
            } else {
                ("[plain]", Style::default().fg(Color::DarkGray))
            };
            let type_label = if var.encrypted {
                var.fhe_type.label()
            } else {
                ""
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "{}{:<22} {:<22} ",
                        prefix,
                        trunc(&var.name, 22),
                        trunc(&var.value, 22)
                    ),
                    bg,
                ),
                Span::styled(format!("{:<10} ", enc_label), enc_style),
                Span::styled(type_label, Style::default().fg(Color::Cyan)),
            ]));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_response_preview(f: &mut Frame, app: &App, area: Rect) {
    let title = match app.response_status {
        Some(s) => format!(" Response [{}] ", s),
        None => " Response ".to_string(),
    };
    let border_color = match app.response_status {
        Some(s) => status_color(s),
        None => Color::DarkGray,
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let text = if app.sending {
        Span::styled("Sending...", Style::default().fg(Color::Yellow))
    } else if app.response_body.is_empty() {
        Span::styled(
            "No response yet. Press [s] to send.",
            Style::default().fg(Color::DarkGray),
        )
    } else {
        let preview: String = app.response_body.chars().take(400).collect();
        Span::raw(preview)
    };

    f.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.sending {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Cyan)
    };
    f.render_widget(
        Paragraph::new(Span::styled(format!(" {}", app.status_msg), style)),
        area,
    );
}

fn draw_help_main(f: &mut Frame, app: &App, area: Rect) {
    let help = if app.mode == Mode::EditUrl {
        " [Enter/Esc] Done  [Left/Right] Cursor  [Home/End] Jump"
    } else {
        " [u] URL  [m] Method  [a] Add  [e] Edit  [d] Del  [s] Send  [r] Resp  [k] ServerKey  [q] Quit"
    };
    f.render_widget(
        Paragraph::new(Span::styled(help, Style::default().fg(Color::DarkGray))),
        area,
    );
}

// ── Add/Edit variable popup ───────────────────────────────────────────────────

fn draw_editvar_popup(f: &mut Frame, app: &App) {
    let height = if app.edit.encrypted { 75 } else { 60 };
    let area = centered_rect(65, height, f.area());
    f.render_widget(Clear, area);

    let title = if app.edit.editing_idx.is_some() {
        " Edit Variable "
    } else {
        " Add Variable "
    };
    let outer = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let type_row_height: u16 = if app.edit.encrypted { 3 } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),              // Name
            Constraint::Length(3),              // Value
            Constraint::Length(3),              // Encrypted toggle
            Constraint::Length(type_row_height), // Type selector (only when encrypted)
            Constraint::Min(1),                 // Help
        ])
        .split(inner);

    // Name
    let name_active = app.edit.field == VarField::Name;
    draw_input_field(
        f,
        " Name ",
        &format!("{}{}", app.edit.name, if name_active { "|" } else { "" }),
        name_active,
        chunks[0],
    );

    // Value
    let val_active = app.edit.field == VarField::Value;
    let val_title = if app.edit.encrypted {
        format!(" Value ({}) ", app.edit.selected_fhe_type().label())
    } else {
        " Value ".to_string()
    };
    draw_input_field(
        f,
        &val_title,
        &format!("{}{}", app.edit.value, if val_active { "|" } else { "" }),
        val_active,
        chunks[1],
    );

    // Encrypted toggle
    let enc_active = app.edit.field == VarField::Encrypted;
    let enc_block = Block::default()
        .title(" Encryption ")
        .borders(Borders::ALL)
        .border_style(if enc_active {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        });
    let enc_text = if app.edit.encrypted {
        Span::styled(
            "[x] TFHE homomorphic encryption",
            Style::default().fg(Color::Green),
        )
    } else {
        Span::styled("[ ] Plain text (no encryption)", Style::default().fg(Color::DarkGray))
    };
    f.render_widget(Paragraph::new(enc_text).block(enc_block), chunks[2]);

    // Type selector (only shown when encrypted)
    if app.edit.encrypted && type_row_height > 0 {
        let type_active = app.edit.field == VarField::FheType;
        let type_block = Block::default()
            .title(" TFHE Type  [Left/Right to change] ")
            .borders(Borders::ALL)
            .border_style(if type_active {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            });

        let type_spans: Vec<Span> = FheType::all()
            .iter()
            .enumerate()
            .flat_map(|(i, t)| {
                let selected = i == app.edit.fhe_type_idx;
                let style = if selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                vec![Span::styled(format!(" {} ", t.label()), style), Span::raw("  ")]
            })
            .collect();

        f.render_widget(
            Paragraph::new(Line::from(type_spans)).block(type_block),
            chunks[3],
        );
    }

    // Help
    let help_text = if app.edit.encrypted {
        " [Tab] Next  [Left/Right] Type  [Space] Toggle  [Enter] Save  [Esc] Cancel"
    } else {
        " [Tab] Next  [Space] Toggle encryption  [Enter] Save  [Esc] Cancel"
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            help_text,
            Style::default().fg(Color::DarkGray),
        )),
        chunks[4],
    );
}

fn draw_input_field(f: &mut Frame, title: &str, value: &str, active: bool, area: Rect) {
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(if active {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        });
    let style = if active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    f.render_widget(Paragraph::new(Span::styled(value, style)).block(block), area);
}

// ── Response / Decrypt screen ─────────────────────────────────────────────────

fn draw_response(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // Response body
            Constraint::Length(8), // Decrypt panel
            Constraint::Length(1), // Help
        ])
        .split(area);

    // Response body
    let status_str = app
        .response_status
        .map(|s| format!(" Response [{}] ", s))
        .unwrap_or_else(|| " Response ".to_string());
    let border_color = app.response_status.map(status_color).unwrap_or(Color::Cyan);

    let display = prettify_json(&app.response_body);
    f.render_widget(
        Paragraph::new(display.as_str())
            .block(
                Block::default()
                    .title(status_str)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color)),
            )
            .wrap(Wrap { trim: false })
            .scroll((app.response_scroll, 0)),
        chunks[0],
    );

    // Decrypt panel
    let dec_block = Block::default()
        .title(" Decrypt Response Fields ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let dec_inner = dec_block.inner(chunks[1]);
    f.render_widget(dec_block, chunks[1]);

    if app.response_fields.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "No JSON object fields found.",
                Style::default().fg(Color::DarkGray),
            )),
            dec_inner,
        );
    } else {
        let panel_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // field selector row
                Constraint::Length(1), // type selector row
                Constraint::Length(1), // result row
                Constraint::Min(0),
            ])
            .split(dec_inner);

        // Field selector
        let field_spans: Vec<Span> = app
            .response_fields
            .iter()
            .enumerate()
            .flat_map(|(i, (k, _))| {
                let sel = i == app.decrypt_selected;
                let style = if sel {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                vec![Span::styled(format!(" {} ", k), style), Span::raw("  ")]
            })
            .collect();
        f.render_widget(Paragraph::new(Line::from(field_spans)), panel_chunks[0]);

        // Type selector
        let type_spans: Vec<Span> = FheType::all()
            .iter()
            .enumerate()
            .flat_map(|(i, t)| {
                let sel = i == app.decrypt_type_idx;
                let style = if sel {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                vec![Span::styled(format!(" {} ", t.label()), style), Span::raw(" ")]
            })
            .collect();
        f.render_widget(
            Paragraph::new(Line::from(
                [vec![Span::styled(
                    "Type: ",
                    Style::default().fg(Color::DarkGray),
                )], type_spans]
                    .concat(),
            )),
            panel_chunks[1],
        );

        // Result
        if let Some((name, value)) = app.response_fields.get(app.decrypt_selected) {
            let (text, style) = if let Some(r) = &app.decrypt_result {
                (
                    format!(" {} = {}", name, r),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                (
                    format!(" {}: {}...", name, trunc(value, 55)),
                    Style::default().fg(Color::DarkGray),
                )
            };
            f.render_widget(
                Paragraph::new(Span::styled(text, style)),
                panel_chunks[2],
            );
        }
    }

    // Help
    f.render_widget(
        Paragraph::new(Span::styled(
            " [Up/Down] Scroll  [Left/Right] Field  [[]]] Type  [d] Decrypt  [Esc] Back",
            Style::default().fg(Color::DarkGray),
        )),
        chunks[2],
    );
}

// ── Server key screen ─────────────────────────────────────────────────────────

fn draw_server_key(f: &mut Frame, app: &App) {
    let area = f.area();
    let outer = Block::default()
        .title(" Server Key (base64) — Send this to your server once ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "The server key lets the server perform homomorphic operations on ciphertexts.",
                Style::default().fg(Color::White),
            )),
            Line::from(Span::styled(
                "It does NOT allow decryption. Only your client key can decrypt.",
                Style::default().fg(Color::Yellow),
            )),
        ]),
        chunks[0],
    );

    let key_preview = app
        .server_key_b64
        .as_deref()
        .unwrap_or("(not available)");

    f.render_widget(
        Paragraph::new(key_preview)
            .block(
                Block::default()
                    .title(" base64 payload ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: true }),
        chunks[1],
    );

    let size_str = if let Some(k) = &app.server_key_b64 {
        format!(" Key size: {} bytes (base64)", k.len())
    } else {
        String::new()
    };
    f.render_widget(
        Paragraph::new(Span::styled(size_str, Style::default().fg(Color::DarkGray))),
        chunks[2],
    );

    f.render_widget(
        Paragraph::new(Span::styled(
            " POST to your server as JSON field 'server_key', or configure it once via /admin  |  [any key] Back",
            Style::default().fg(Color::DarkGray),
        )),
        chunks[3],
    );
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn trunc(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let end = s
            .char_indices()
            .nth(max.saturating_sub(3))
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}...", &s[..end])
    } else {
        s.to_string()
    }
}

fn status_color(status: u16) -> Color {
    match status {
        200..=299 => Color::Green,
        300..=399 => Color::Yellow,
        _ => Color::Red,
    }
}

fn prettify_json(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .and_then(|v| serde_json::to_string_pretty(&v).map_err(Into::into))
        .unwrap_or_else(|_| body.to_string())
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}
