//! Visual language for the native desktop app: "midnight studio".
//!
//! The palette intentionally avoids a generic purple SaaS look. Large areas are
//! ink-blue, the only warm accent is the starlight gold used for the next
//! production step, and borders carry most of the hierarchy.

use iced::font::Weight;
use iced::widget::{button, container, pick_list, text, text_editor, text_input};
use iced::{Border, Color, Element, Font, Shadow, Theme};

pub const INK: Color = Color::from_rgb8(0x0A, 0x0F, 0x1A);
pub const RAIL: Color = Color::from_rgb8(0x0C, 0x13, 0x20);
pub const SURFACE: Color = Color::from_rgb8(0x11, 0x1A, 0x29);
pub const SURFACE_HIGH: Color = Color::from_rgb8(0x16, 0x21, 0x33);
pub const SURFACE_SOFT: Color = Color::from_rgb8(0x0D, 0x15, 0x24);
pub const LINE: Color = Color::from_rgb8(0x23, 0x31, 0x4A);
pub const LINE_SOFT: Color = Color::from_rgb8(0x1A, 0x25, 0x38);
pub const TEXT: Color = Color::from_rgb8(0xEE, 0xF2, 0xF9);
pub const TEXT_SECONDARY: Color = Color::from_rgb8(0xA7, 0xB5, 0xCA);
pub const TEXT_MUTED: Color = Color::from_rgb8(0x6E, 0x7F, 0x96);
pub const GOLD: Color = Color::from_rgb8(0xE6, 0xB3, 0x6B);
pub const GOLD_BRIGHT: Color = Color::from_rgb8(0xF2, 0xC8, 0x88);
pub const GOLD_DIM: Color = Color::from_rgb8(0x6A, 0x52, 0x32);
pub const GOLD_SOFT: Color = Color::from_rgb8(0x2A, 0x21, 0x16);
pub const INDIGO: Color = Color::from_rgb8(0x82, 0x90, 0xF6);
pub const MINT: Color = Color::from_rgb8(0x6F, 0xD3, 0xA8);
pub const CORAL: Color = Color::from_rgb8(0xE9, 0x8B, 0x8B);
pub const INK_ON_GOLD: Color = Color::from_rgb8(0x1B, 0x13, 0x08);

pub fn theme() -> Theme {
    Theme::custom(
        "星晴 · 午夜片场",
        iced::theme::Palette {
            background: INK,
            text: TEXT,
            primary: GOLD,
            success: MINT,
            danger: CORAL,
            warning: GOLD,
        },
    )
}

pub fn font(weight: Weight) -> Font {
    Font {
        weight,
        ..Font::with_name(if cfg!(target_os = "macos") {
            "PingFang SC"
        } else {
            "Microsoft YaHei"
        })
    }
}

pub fn display<'a>(value: impl Into<String>) -> text::Text<'a> {
    text(value.into())
        .size(30)
        .font(font(Weight::Semibold))
        .color(TEXT)
}

pub fn title<'a>(value: impl Into<String>) -> text::Text<'a> {
    text(value.into())
        .size(24)
        .font(font(Weight::Semibold))
        .color(TEXT)
}

pub fn heading<'a>(value: impl Into<String>) -> text::Text<'a> {
    text(value.into())
        .size(17)
        .font(font(Weight::Semibold))
        .color(TEXT)
}

pub fn muted<'a>(value: impl Into<String>) -> text::Text<'a> {
    text(value.into())
        .size(12)
        .color(TEXT_SECONDARY)
        .line_height(1.6)
}

pub fn eyebrow<'a>(value: impl Into<String>) -> text::Text<'a> {
    text(value.into())
        .size(10)
        .font(font(Weight::Medium))
        .color(TEXT_MUTED)
}

pub fn number<'a>(value: impl Into<String>) -> text::Text<'a> {
    text(value.into())
        .size(24)
        .font(font(Weight::Semibold))
        .color(TEXT)
}

pub fn app(_: &Theme) -> container::Style {
    container::Style {
        background: Some(INK.into()),
        ..container::Style::default()
    }
}

pub fn rail(_: &Theme) -> container::Style {
    container::Style {
        background: Some(RAIL.into()),
        border: Border {
            color: LINE_SOFT,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn topbar(_: &Theme) -> container::Style {
    container::Style {
        background: Some(RAIL.into()),
        border: Border {
            color: LINE_SOFT,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn statusbar(_: &Theme) -> container::Style {
    container::Style {
        background: Some(RAIL.into()),
        border: Border {
            color: LINE_SOFT,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn panel(_: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE.into()),
        border: Border {
            color: LINE_SOFT,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn card(_: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_SOFT.into()),
        border: Border {
            color: LINE_SOFT,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn banner(_: &Theme) -> container::Style {
    container::Style {
        background: Some(GOLD_SOFT.into()),
        border: Border {
            color: GOLD_DIM,
            width: 1.0,
            radius: 10.0.into(),
        },
        text_color: Some(GOLD_BRIGHT),
        ..container::Style::default()
    }
}

pub fn danger_banner(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba8(0xE9, 0x8B, 0x8B, 0.08).into()),
        border: Border {
            color: Color::from_rgba8(0xE9, 0x8B, 0x8B, 0.35),
            width: 1.0,
            radius: 10.0.into(),
        },
        text_color: Some(CORAL),
        ..container::Style::default()
    }
}

/// A panel whose background is painted as a sibling instead of an ancestor.
///
/// Iced 0.14's text editor content is composited in a separate text layer; an
/// ancestor container background hides it. Drawing the surface behind the
/// content keeps editable text visible without giving up the dark panels.
pub fn panel_surface<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    iced::widget::stack![
        container(iced::widget::space::horizontal())
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .style(panel),
        content.into(),
    ]
    .width(iced::Length::Fill)
    .height(iced::Length::Fill)
    .into()
}

pub fn divider<'a, M: 'a>() -> Element<'a, M> {
    container(iced::widget::space::horizontal())
        .height(1)
        .width(iced::Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(LINE_SOFT.into()),
            ..container::Style::default()
        })
        .into()
}

#[derive(Debug, Clone, Copy)]
pub enum Tone {
    Gold,
    Mint,
    Indigo,
    Coral,
    Muted,
}

impl Tone {
    pub fn colors(self) -> (Color, Color) {
        match self {
            Tone::Gold => (GOLD_SOFT, GOLD_BRIGHT),
            Tone::Mint => (Color::from_rgba8(0x6F, 0xD3, 0xA8, 0.10), MINT),
            Tone::Indigo => (Color::from_rgba8(0x82, 0x90, 0xF6, 0.10), INDIGO),
            Tone::Coral => (Color::from_rgba8(0xE9, 0x8B, 0x8B, 0.10), CORAL),
            Tone::Muted => (SURFACE_HIGH, TEXT_SECONDARY),
        }
    }
}

pub fn chip<'a, M: 'a>(value: impl Into<String>, tone: Tone) -> Element<'a, M> {
    let (background, foreground) = tone.colors();
    container(text(value.into()).size(11).color(foreground))
        .padding([4, 8])
        .style(move |_: &Theme| container::Style {
            background: Some(background.into()),
            border: Border {
                color: background,
                width: 1.0,
                radius: 999.0.into(),
            },
            text_color: Some(foreground),
            ..container::Style::default()
        })
        .into()
}

fn button_style(background: Color, border: Color, foreground: Color) -> button::Style {
    button::Style {
        background: Some(background.into()),
        text_color: foreground,
        border: Border {
            color: border,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn primary(_: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Active => button_style(GOLD, GOLD, INK_ON_GOLD),
        button::Status::Hovered => button_style(GOLD_BRIGHT, GOLD_BRIGHT, INK_ON_GOLD),
        button::Status::Pressed => button_style(
            Color::from_rgb8(0xD0, 0x9D, 0x58),
            Color::from_rgb8(0xD0, 0x9D, 0x58),
            INK_ON_GOLD,
        ),
        button::Status::Disabled => button_style(SURFACE_SOFT, LINE_SOFT, TEXT_MUTED),
    }
}

pub fn secondary(_: &Theme, status: button::Status) -> button::Style {
    let (background, border, foreground) = match status {
        button::Status::Hovered => (SURFACE_HIGH, LINE, TEXT),
        button::Status::Pressed => (SURFACE_SOFT, GOLD_DIM, TEXT),
        button::Status::Disabled => (Color::TRANSPARENT, LINE_SOFT, TEXT_MUTED),
        button::Status::Active => (Color::TRANSPARENT, LINE, TEXT_SECONDARY),
    };
    button::Style {
        background: Some(background.into()),
        text_color: foreground,
        border: Border {
            color: border,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn ghost(_: &Theme, status: button::Status) -> button::Style {
    let (background, foreground) = match status {
        button::Status::Hovered | button::Status::Pressed => (SURFACE_HIGH, TEXT),
        button::Status::Disabled => (Color::TRANSPARENT, TEXT_MUTED),
        button::Status::Active => (Color::TRANSPARENT, TEXT_SECONDARY),
    };
    button::Style {
        background: Some(background.into()),
        text_color: foreground,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn danger(_: &Theme, status: button::Status) -> button::Style {
    let (background, border) = match status {
        button::Status::Hovered | button::Status::Pressed => {
            (Color::from_rgba8(0xE9, 0x8B, 0x8B, 0.12), CORAL)
        }
        button::Status::Disabled => (Color::TRANSPARENT, LINE_SOFT),
        button::Status::Active => (
            Color::TRANSPARENT,
            Color::from_rgba8(0xE9, 0x8B, 0x8B, 0.45),
        ),
    };
    button::Style {
        background: Some(background.into()),
        text_color: if matches!(status, button::Status::Disabled) {
            TEXT_MUTED
        } else {
            CORAL
        },
        border: Border {
            color: border,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn nav(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_, status| {
        let (background, foreground) = if active {
            (SURFACE_HIGH, GOLD_BRIGHT)
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => (SURFACE, TEXT),
                button::Status::Disabled => (Color::TRANSPARENT, TEXT_MUTED),
                button::Status::Active => (Color::TRANSPARENT, TEXT_SECONDARY),
            }
        };
        button::Style {
            background: Some(background.into()),
            text_color: foreground,
            border: Border {
                color: if active {
                    LINE_SOFT
                } else {
                    Color::TRANSPARENT
                },
                width: 1.0,
                radius: 8.0.into(),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn field(_: &Theme, status: text_input::Status) -> text_input::Style {
    let border = match status {
        text_input::Status::Focused { .. } => GOLD_DIM,
        text_input::Status::Hovered => LINE,
        _ => LINE_SOFT,
    };
    text_input::Style {
        background: INK.into(),
        border: Border {
            color: border,
            width: 1.0,
            radius: 8.0.into(),
        },
        icon: TEXT_MUTED,
        placeholder: TEXT_MUTED,
        value: TEXT,
        selection: Color::from_rgba8(0xE6, 0xB3, 0x6B, 0.30),
    }
}

pub fn title_field(_: &Theme, status: text_input::Status) -> text_input::Style {
    let border = match status {
        text_input::Status::Focused { .. } => GOLD_DIM,
        _ => Color::TRANSPARENT,
    };
    text_input::Style {
        background: Color::TRANSPARENT.into(),
        border: Border {
            color: border,
            width: 1.0,
            radius: 6.0.into(),
        },
        icon: TEXT_MUTED,
        placeholder: TEXT_MUTED,
        value: TEXT,
        selection: Color::from_rgba8(0xE6, 0xB3, 0x6B, 0.30),
    }
}

pub fn editor(_: &Theme, status: text_editor::Status) -> text_editor::Style {
    let border = match status {
        text_editor::Status::Focused { .. } => GOLD_DIM,
        text_editor::Status::Hovered => LINE,
        _ => LINE_SOFT,
    };
    text_editor::Style {
        background: SURFACE_SOFT.into(),
        border: Border {
            color: border,
            width: 1.0,
            radius: 10.0.into(),
        },
        placeholder: TEXT_MUTED,
        value: TEXT,
        selection: Color::from_rgba8(0xE6, 0xB3, 0x6B, 0.28),
    }
}

pub fn pick(_: &Theme, status: pick_list::Status) -> pick_list::Style {
    let border = match status {
        pick_list::Status::Hovered | pick_list::Status::Opened { .. } => LINE,
        _ => LINE_SOFT,
    };
    pick_list::Style {
        text_color: TEXT,
        placeholder_color: TEXT_MUTED,
        handle_color: TEXT_MUTED,
        background: INK.into(),
        border: Border {
            color: border,
            width: 1.0,
            radius: 8.0.into(),
        },
    }
}

pub fn scrollable(
    _: &Theme,
    _: iced::widget::scrollable::Status,
) -> iced::widget::scrollable::Style {
    let rail = iced::widget::scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: iced::widget::scrollable::Scroller {
            background: LINE.into(),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 999.0.into(),
            },
        },
    };
    iced::widget::scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: iced::widget::scrollable::AutoScroll {
            background: Color::TRANSPARENT.into(),
            border: Border::default(),
            shadow: Shadow::default(),
            icon: TEXT_MUTED,
        },
    }
}
