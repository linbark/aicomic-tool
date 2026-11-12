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
fn hex(v: u32) -> Color {
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
