//! Capture application shortcuts before focused controls receive their text.
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

fn capture_save<Message: Clone>(
    event: &Event,
    on_save: &Message,
    shell: &mut Shell<'_, Message>,
) -> bool {
    if let Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key,
        physical_key,
        modifiers,
        ..
    }) = event
        && modifiers.command()
        && key.to_latin(*physical_key) == Some('s')
    {
        shell.publish(on_save.clone());
        shell.capture_event();
        true
    } else {
        false
    }
}

pub fn save<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    on_save: Message,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    Element::new(SaveShortcut {
        content: content.into(),
        on_save,
    })
}

struct SaveShortcut<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    on_save: Message,
}

impl<Message: Clone, Theme, Renderer: renderer::Renderer> Widget<Message, Theme, Renderer>
    for SaveShortcut<'_, Message, Theme, Renderer>
{
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
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
        if capture_save(event, &self.on_save, shell) {
            return;
        }
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
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
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
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
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
            .map(|content| {
                overlay::Element::new(Box::new(SaveOverlay {
                    content,
                    on_save: &self.on_save,
                }))
            })
    }
}

// Overlays receive events before the base widget. Keep the same guard around
// menus and nested overlays so a captured shortcut cannot be handled twice.
struct SaveOverlay<'a, Message, Theme, Renderer> {
    content: overlay::Element<'a, Message, Theme, Renderer>,
    on_save: &'a Message,
}
impl<Message: Clone, Theme, Renderer: renderer::Renderer> overlay::Overlay<Message, Theme, Renderer>
    for SaveOverlay<'_, Message, Theme, Renderer>
{
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        self.content.as_overlay_mut().layout(renderer, bounds)
    }
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.content
            .as_overlay()
            .draw(renderer, theme, style, layout, cursor);
    }
    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content
            .as_overlay_mut()
            .operate(layout, renderer, operation);
    }
    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        if capture_save(event, self.on_save, shell) {
            return;
        }
        self.content
            .as_overlay_mut()
            .update(event, layout, cursor, renderer, clipboard, shell);
    }
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_overlay()
            .mouse_interaction(layout, cursor, renderer)
    }
    fn overlay<'b>(
        &'b mut self,
        layout: Layout<'b>,
        renderer: &Renderer,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_overlay_mut()
            .overlay(layout, renderer)
            .map(|content| {
                overlay::Element::new(Box::new(SaveOverlay {
                    content,
                    on_save: self.on_save,
                }))
            })
    }
    fn index(&self) -> f32 {
        self.content.as_overlay().index()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::clipboard;
    use iced::keyboard::{self, Key, Location, Modifiers, key};
    use std::{cell::Cell, rc::Rc};

    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Save,
        ChildText(String),
    }

    struct Spy {
        received: Rc<Cell<usize>>,
        with_overlay: bool,
    }
    fn receive(event: &Event, received: &Cell<usize>, shell: &mut Shell<'_, Message>) {
        received.set(received.get() + 1);
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            text: Some(text), ..
        }) = event
        {
            shell.publish(Message::ChildText(text.to_string()));
        }
    }
    impl Widget<Message, (), ()> for Spy {
        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Fill)
        }
        fn layout(&mut self, _: &mut Tree, _: &(), _: &layout::Limits) -> layout::Node {
            layout::Node::new(Size::new(10.0, 10.0))
        }
        fn draw(
            &self,
            _: &Tree,
            _: &mut (),
            _: &(),
            _: &renderer::Style,
            _: Layout<'_>,
            _: mouse::Cursor,
            _: &Rectangle,
        ) {
        }
        fn update(
            &mut self,
            _: &mut Tree,
            event: &Event,
            _: Layout<'_>,
            _: mouse::Cursor,
            _: &(),
            _: &mut dyn Clipboard,
            shell: &mut Shell<'_, Message>,
            _: &Rectangle,
        ) {
            receive(event, &self.received, shell);
        }
        fn overlay<'b>(
            &'b mut self,
            _: &'b mut Tree,
            _: Layout<'b>,
            _: &(),
            _: &Rectangle,
            _: Vector,
        ) -> Option<overlay::Element<'b, Message, (), ()>> {
            self.with_overlay
                .then(|| overlay::Element::new(Box::new(SpyOverlay(self.received.clone()))))
        }
    }
    struct SpyOverlay(Rc<Cell<usize>>);
    impl overlay::Overlay<Message, (), ()> for SpyOverlay {
        fn layout(&mut self, _: &(), _: Size) -> layout::Node {
            layout::Node::new(Size::new(10.0, 10.0))
        }
        fn draw(&self, _: &mut (), _: &(), _: &renderer::Style, _: Layout<'_>, _: mouse::Cursor) {}
        fn update(
            &mut self,
            event: &Event,
            _: Layout<'_>,
            _: mouse::Cursor,
            _: &(),
            _: &mut dyn Clipboard,
            shell: &mut Shell<'_, Message>,
        ) {
            receive(event, &self.0, shell);
        }
    }
    fn pressed(text: &str, modifiers: Modifiers) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Character(text.into()),
            modified_key: Key::Character(text.into()),
            physical_key: key::Physical::Code(key::Code::KeyS),
            location: Location::Standard,
            modifiers,
            text: Some(text.into()),
            repeat: false,
        })
    }
    fn dispatch(event: &Event, with_overlay: bool) -> (usize, Vec<Message>, bool) {
        let received = Rc::new(Cell::new(0));
        let child: Element<'_, Message, (), ()> = Element::new(Spy {
            received: received.clone(),
            with_overlay,
        });
        let mut widget = save(child, Message::Save);
        let mut tree = Tree::new(widget.as_widget());
        let node = layout::Node::new(Size::new(10.0, 10.0));
        let viewport = Rectangle::with_size(Size::new(10.0, 10.0));
        let mut messages = vec![];
        let captured = {
            let mut shell = Shell::new(&mut messages);
            if with_overlay {
                widget
                    .as_widget_mut()
                    .overlay(&mut tree, Layout::new(&node), &(), &viewport, Vector::ZERO)
                    .unwrap()
                    .as_overlay_mut()
                    .update(
                        event,
                        Layout::new(&node),
                        mouse::Cursor::Unavailable,
                        &(),
                        &mut clipboard::Null,
                        &mut shell,
                    );
            } else {
                widget.as_widget_mut().update(
                    &mut tree,
                    event,
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &(),
                    &mut clipboard::Null,
                    &mut shell,
                    &viewport,
                );
            }
            shell.is_event_captured()
        };
        (received.get(), messages, captured)
    }
    #[test]
    fn save_command_is_published_once_and_never_reaches_the_focused_control() {
        let (received, messages, captured) = dispatch(&pressed("s", Modifiers::COMMAND), false);
        assert_eq!(
            received, 0,
            "a save shortcut must not insert its text into a control"
        );
        assert_eq!(messages, vec![Message::Save]);
        assert!(captured);
    }
    #[test]
    fn ordinary_text_keeps_reaching_the_focused_control() {
        let (received, messages, captured) = dispatch(&pressed("s", Modifiers::empty()), false);
        assert_eq!(received, 1);
        assert_eq!(messages, vec![Message::ChildText("s".into())]);
        assert!(!captured);
    }
    #[test]
    fn save_is_also_captured_before_an_overlay_receives_its_text() {
        let (received, messages, captured) = dispatch(&pressed("s", Modifiers::COMMAND), true);
        assert_eq!(received, 0);
        assert_eq!(messages, vec![Message::Save]);
        assert!(captured);
    }
    #[test]
    fn ordinary_overlay_text_is_forwarded() {
        let (received, messages, captured) = dispatch(&pressed("s", Modifiers::empty()), true);
        assert_eq!(received, 1);
        assert_eq!(messages, vec![Message::ChildText("s".into())]);
        assert!(!captured);
    }
    #[test]
    fn save_uses_the_physical_latin_key_under_a_non_latin_layout() {
        let (received, messages, captured) = dispatch(&pressed("ы", Modifiers::COMMAND), false);
        assert_eq!(received, 0);
        assert_eq!(messages, vec![Message::Save]);
        assert!(captured);
    }
}
