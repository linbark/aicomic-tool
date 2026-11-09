//! A text editor that paints its own content on top of Iced's native editor.
//!
//! Iced 0.14 renders editor content in a separate text layer that can be hidden
//! by ancestor backgrounds or nested layouts on macOS. The native widget still
//! handles layout, focus, IME, selection and editing; this wrapper only adds a
//! normal paragraph of the same text so the user can actually see it. The size
//! and line height passed to the native editor and the overlay must match.

use iced::advanced::text::highlighter::PlainText;
use iced::advanced::text::{self, LineHeight, Renderer as _, Wrapping};
use iced::advanced::widget::{self, Operation, Tree, Widget};
use iced::advanced::{Clipboard, Layout, Shell, renderer};
use iced::widget::text_editor::{self, TextEditor};
use iced::{Color, Element, Event, Length, Rectangle, Renderer, Size, Theme, mouse};

pub struct ReadableEditor<'a, Message> {
    inner: TextEditor<'a, PlainText, Message>,
    text: String,
    padding: f32,
    size: f32,
    line_height: f32,
    color: Color,
}

impl<'a, Message> ReadableEditor<'a, Message> {
    pub fn new(inner: TextEditor<'a, PlainText, Message>, text: String) -> Self {
        Self {
            inner,
            text,
            padding: 0.0,
            size: 16.0,
            line_height: 20.8,
            color: Color::BLACK,
        }
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

impl<'a, Message> ReadableEditor<'a, Message>
where
    Message: 'a,
{
    pub fn into_element(self) -> Element<'a, Message> {
        Element::new(self)
    }
}

pub fn dark_editor<'a, Message>(
    content: &'a text_editor::Content,
    height: impl Into<Length>,
    on_action: impl Fn(text_editor::Action) -> Message + 'a,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let inner = iced::widget::text_editor(content)
        .height(height)
        .padding(10)
        .on_action(on_action)
        .style(crate::ui::editor);
    ReadableEditor::new(inner, content.text())
        .padding(10.0)
        .color(crate::ui::TEXT)
        .into_element()
}

impl<Message> Widget<Message, Theme, Renderer> for ReadableEditor<'_, Message> {
    fn tag(&self) -> widget::tree::Tag {
        Widget::tag(&self.inner)
    }

    fn state(&self) -> widget::tree::State {
        Widget::state(&self.inner)
    }

    fn children(&self) -> Vec<Tree> {
        Widget::children(&self.inner)
    }

    fn diff(&self, tree: &mut Tree) {
        Widget::diff(&self.inner, tree);
    }

    fn size(&self) -> Size<Length> {
        Widget::size(&self.inner)
    }

    fn size_hint(&self) -> Size<Length> {
        Widget::size_hint(&self.inner)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        Widget::layout(&mut self.inner, tree, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        Widget::operate(&mut self.inner, tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        Widget::update(
            &mut self.inner,
            tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        Widget::draw(
            &self.inner,
            tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        if self.text.is_empty() {
            return;
        }
        let bounds = layout.bounds();
        if !bounds.intersects(viewport) {
            return;
        }
        let text_bounds = bounds.shrink(self.padding);
        let Some(clip) = text_bounds.intersection(viewport) else {
            return;
        };
        renderer.fill_text(
            text::Text {
                content: self.text.clone(),
                bounds: Size::new(text_bounds.width, text_bounds.height),
                size: self.size.into(),
                line_height: LineHeight::Absolute(self.line_height.into()),
                font: renderer.default_font(),
                align_x: text::Alignment::Default,
                align_y: iced::alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping: Wrapping::WordOrGlyph,
            },
            text_bounds.position(),
            self.color,
            clip,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        Widget::mouse_interaction(&self.inner, tree, layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: iced::Vector,
    ) -> Option<iced::overlay::Element<'b, Message, Theme, Renderer>> {
        Widget::overlay(
            &mut self.inner,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
