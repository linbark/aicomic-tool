use iced::widget::{button, container, text};
use iced::{Border, Color, Theme};

#[derive(Clone, Copy)]
pub struct Colors {
    pub background: Color,
    pub surface: Color,
    pub sidebar: Color,
    pub inset: Color,
    pub text: Color,
    pub secondary: Color,
    pub line: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub selected: Color,
    pub error: Color,
}
const fn hex(v: u32) -> Color {
    Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
pub fn colors(theme: &Theme) -> Colors {
    let p = theme.palette();
    if p.background.r + p.background.g + p.background.b < 1.5 {
        Colors {
            background: hex(0x202122),
            surface: hex(0x28292B),
            sidebar: hex(0x252628),
            inset: hex(0x323335),
            text: hex(0xF1F0ED),
            secondary: hex(0xB4B2AD),
            line: hex(0x454649),
            accent: hex(0xD9AE64),
            accent_text: hex(0x292216),
            selected: hex(0x3D3931),
            error: hex(0xF1A69D),
        }
    } else {
        Colors {
            background: hex(0xFAFAF8),
            surface: hex(0xFFFFFF),
            sidebar: hex(0xF0F0ED),
            inset: hex(0xF3F3F0),
            text: hex(0x282A2D),
            secondary: hex(0x62645F),
            line: hex(0xDDDED8),
            accent: hex(0xC9A15A),
            accent_text: hex(0x292216),
            selected: hex(0xE9E8E2),
            error: hex(0x9D3F32),
        }
    }
}
pub fn root(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.background.into()),
        text_color: Some(c.text),
        ..Default::default()
    }
}
pub fn surface(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.surface.into()),
        text_color: Some(c.text),
        border: Border {
            color: c.line,
            width: 1.,
            radius: 8.into(),
        },
        ..Default::default()
    }
}
pub fn sidebar(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.sidebar.into()),
        text_color: Some(c.text),
        ..Default::default()
    }
}
pub fn inset(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.inset.into()),
        text_color: Some(c.text),
        border: Border {
            radius: 6.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
pub fn secondary(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(colors(theme).secondary),
    }
}
pub fn error_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(colors(theme).error),
    }
}
pub fn normal(theme: &Theme, status: button::Status) -> button::Style {
    button_style(theme, status, false, false)
}
pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    button_style(theme, status, true, false)
}
pub fn selected(theme: &Theme, status: button::Status) -> button::Style {
    button_style(theme, status, false, true)
}
pub fn quiet(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    button::Style {
        background: (status == button::Status::Hovered || status == button::Status::Pressed)
            .then_some(c.selected.into()),
        text_color: if status == button::Status::Disabled {
            c.secondary
        } else {
            c.text
        },
        border: Border {
            radius: 6.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
fn button_style(
    theme: &Theme,
    status: button::Status,
    primary: bool,
    selected: bool,
) -> button::Style {
    let c = colors(theme);
    let disabled = status == button::Status::Disabled;
    let hover = status == button::Status::Hovered || status == button::Status::Pressed;
    let bg = if disabled {
        c.inset
    } else if primary {
        c.accent
    } else if selected || hover {
        c.selected
    } else {
        c.surface
    };
    button::Style {
        background: Some(bg.into()),
        text_color: if disabled {
            c.secondary
        } else if primary {
            c.accent_text
        } else {
            c.text
        },
        border: Border {
            color: if primary || selected { bg } else { c.line },
            width: 1.,
            radius: 6.into(),
        },
        ..Default::default()
    }
}

/// v2 material palette: dark green workshop, old copper actions, legible paper editors.
/// No animation; graph lines convey actual dependencies rather than decoration.
pub mod game {
    use super::*;
    use iced::widget::{text_editor, text_input};
    pub const INK: Color = hex(0x142E29);
    pub const DEEP: Color = hex(0x10251F);
    pub const PANEL: Color = hex(0x1B3630);
    pub const MUTED: Color = hex(0xB7C5B8);
    pub const PAPER: Color = hex(0xEEE5D1);
    pub const PAPER_LIGHT: Color = hex(0xF7EFDF);
    pub const PAPER_INK: Color = hex(0x293A31);
    pub const PAPER_MUTED: Color = hex(0x665D4D);
    pub const COPPER: Color = hex(0xC49A60);
    pub const COPPER_LIGHT: Color = hex(0xE0B97E);
    pub const ERROR: Color = hex(0xF3B6A6);
    pub fn root(_: &Theme) -> container::Style {
        container::Style {
            background: Some(INK.into()),
            text_color: Some(PAPER),
            ..Default::default()
        }
    }
    pub fn rail(_: &Theme) -> container::Style {
        container::Style {
            background: Some(DEEP.into()),
            text_color: Some(PAPER),
            ..Default::default()
        }
    }
    pub fn panel(_: &Theme) -> container::Style {
        container::Style {
            background: Some(PANEL.into()),
            text_color: Some(PAPER),
            border: Border {
                color: Color { a: 0.35, ..COPPER },
                width: 1.,
                radius: 6.into(),
            },
            ..Default::default()
        }
    }
    pub fn paper(_: &Theme) -> container::Style {
        container::Style {
            background: Some(PAPER.into()),
            text_color: Some(PAPER_INK),
            border: Border {
                color: COPPER,
                width: 1.,
                radius: 6.into(),
            },
            ..Default::default()
        }
    }
    pub fn button(_: &Theme, status: button::Status) -> button::Style {
        let disabled = status == button::Status::Disabled;
        let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(if hover { INK } else { PANEL }.into()),
            text_color: if disabled { MUTED } else { PAPER },
            border: Border {
                color: if hover {
                    COPPER_LIGHT
                } else {
                    Color { a: 0.45, ..COPPER }
                },
                width: 1.,
                radius: 4.into(),
            },
            ..Default::default()
        }
    }
    pub fn primary(_: &Theme, status: button::Status) -> button::Style {
        let mut s = button(&Theme::Dark, status);
        s.background = Some(
            if status == button::Status::Disabled {
                PANEL
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                COPPER_LIGHT
            } else {
                COPPER
            }
            .into(),
        );
        s.text_color = if status == button::Status::Disabled {
            MUTED
        } else {
            DEEP
        };
        s
    }
    pub fn selected(_: &Theme, status: button::Status) -> button::Style {
        let mut s = button(&Theme::Dark, status);
        s.border.color = COPPER_LIGHT;
        s.border.width = 2.;
        s
    }
    pub fn paper_button(_: &Theme, status: button::Status) -> button::Style {
        let mut s = button(&Theme::Dark, status);
        s.background = Some(
            if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                PAPER_LIGHT
            } else {
                PAPER
            }
            .into(),
        );
        s.text_color = if status == button::Status::Disabled {
            PAPER_MUTED
        } else {
            PAPER_INK
        };
        s
    }
    pub fn input(_: &Theme, status: text_input::Status) -> text_input::Style {
        text_input::Style {
            background: PAPER_LIGHT.into(),
            border: Border {
                color: if matches!(status, text_input::Status::Focused { .. }) {
                    COPPER
                } else {
                    hex(0xB8AC91)
                },
                width: if matches!(status, text_input::Status::Focused { .. }) {
                    2.
                } else {
                    1.
                },
                radius: 4.into(),
            },
            icon: PAPER_MUTED,
            placeholder: PAPER_MUTED,
            value: PAPER_INK,
            selection: COPPER_LIGHT,
        }
    }
    pub fn editor(_: &Theme, status: text_editor::Status) -> text_editor::Style {
        text_editor::Style {
            background: PAPER_LIGHT.into(),
            border: Border {
                color: if matches!(status, text_editor::Status::Focused { .. }) {
                    COPPER
                } else {
                    hex(0xB8AC91)
                },
                width: if matches!(status, text_editor::Status::Focused { .. }) {
                    2.
                } else {
                    1.
                },
                radius: 4.into(),
            },
            placeholder: PAPER_MUTED,
            value: PAPER_INK,
            selection: COPPER_LIGHT,
        }
    }
}
