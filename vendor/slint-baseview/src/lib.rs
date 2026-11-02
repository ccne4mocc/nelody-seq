//! A [baseview](https://github.com/RustAudio/baseview) backend for
//! [Slint](https://slint.dev).
//!
//! This crate lets you run a Slint UI inside a baseview window — most usefully,
//! the editor window of an audio plugin. It owns the baseview window, sets up an
//! OpenGL + FemtoVG renderer, installs a Slint [`Platform`](slint::platform::Platform),
//! and drives the render loop, translating baseview input events into Slint
//! events.
//!
//! It knows nothing about any particular plugin framework. GUI-framework
//! integrations (such as `nice-plug-slint`) build on top of it by supplying the
//! per-frame `update` closure and parenting it into the host window.
//!
//! # Overview
//!
//! Open a window with [`SlintWindow::open_parented`], passing:
//! - a component factory (`|| MyComponent::new()`),
//! - a `setup` closure run once after the component is shown,
//! - an `update` closure run every frame.
//!
//! Both closures receive a [`SlintWindowHandler`] (access to the component, the
//! Slint window, resizing, etc.) and the baseview [`Window`].
//!
//! ```rust,ignore
//! use slint_baseview::{SlintWindow, WindowOpenOptions, WindowScalePolicy, Size};
//!
//! let handle = SlintWindow::open_parented(
//!     &parent,
//!     options,
//!     || MyComponent::new(),
//!     |_handler, _window| { /* one-time setup */ },
//!     |handler, _window| {
//!         handler.component().set_some_value(42.0);
//!     },
//! );
//! ```

mod translate;

use std::{cell::RefCell, rc::Rc, sync::Arc};

use baseview::{Event, WindowEvent as BaseviewWindowEvent, WindowInfo};
use once_cell::unsync::OnceCell;
use slint::platform::femtovg_renderer::FemtoVGRenderer;
use slint::platform::WindowAdapter;
use slint::platform::WindowEvent;
use slint::{LogicalPosition, PhysicalSize, SharedString};

// Re-exports so downstream crates can name these types without depending on
// baseview / slint directly (and stay version-locked with this crate). These are
// also used throughout this module (so they double as the local imports).
pub use baseview::{
    self, gl::GlConfig, DropData, DropEffect, EventStatus, MouseEvent, Size, Window, WindowHandle,
    WindowOpenOptions, WindowScalePolicy,
};
pub use slint;
pub use slint::PlatformError;

/// Best-effort diagnostics for native editor startup. Never let logging failures
/// interfere with the audio host or plugin UI lifecycle.
fn log_backend(message: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("nelody-sequencer-baseview.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", message);
    }
}
// The trait a parent window handle must implement to be parented into baseview.
// This is raw-window-handle **0.5**'s trait (what baseview 0.1.x consumes), even
// though slint itself uses 0.6 internally. Integrations implement this on their
// own parent-handle type.
pub use raw_window_handle::HasRawWindowHandle;

/// A sensible default OpenGL configuration for the FemtoVG renderer: an RGBA8
/// sRGB double-buffered 3.2 context with a 24-bit depth and 8-bit stencil
/// buffer. Use this for [`WindowOpenOptions::gl_config`] unless you have a reason
/// to customize it.
pub fn default_gl_config() -> GlConfig {
    GlConfig {
        version: (3, 2),
        red_bits: 8,
        blue_bits: 8,
        green_bits: 8,
        alpha_bits: 8,
        depth_bits: 24,
        stencil_bits: 8,
        samples: None,
        srgb: true,
        double_buffer: true,
        vsync: false,
        ..Default::default()
    }
}

type SetupHandler<T> = dyn Fn(&SlintWindowHandler<T>, &mut Window) + Send + Sync + 'static;
type UpdateHandler<T> = dyn Fn(&SlintWindowHandler<T>, &mut Window) + Send + Sync + 'static;
type ResizeHandler = dyn Fn(u32, u32) + Send + Sync + 'static;

/// OpenGL interface implementation for baseview.
///
/// Delegates all GL symbol resolution to baseview's `GlContext::get_proc_address`,
/// which handles the platform-specific details (WGL on Windows, dlsym on Unix).
///
/// The inner function is stored in an `Arc` so this type can be cheaply cloned
/// when the `FemtoVGRenderer` is created from inside `WindowAdapter::renderer()`.
#[derive(Clone)]
struct BaseviewOpenGLInterface {
    get_proc_address: Arc<dyn Fn(&str) -> *const core::ffi::c_void + Send + Sync>,
}

impl BaseviewOpenGLInterface {
    fn new(window: &baseview::Window) -> Self {
        // Store the GlContext address as a plain usize so that the closure is Send + Sync.
        //
        // SAFETY: The `GlContext` is owned by the `Window` and lives as long as the window is
        // open.  The `FemtoVGRenderer` (and therefore this interface) is dropped before the
        // window closes, so the pointer is valid for the entire lifetime of the renderer.
        // We only dereference it on the GUI thread (inside FemtoVG's GL loader callback).
        let ctx_addr = window
            .gl_context()
            .expect("window must have an OpenGL context")
            as *const baseview::gl::GlContext as usize;

        Self {
            get_proc_address: Arc::new(move |name: &str| {
                let ctx = ctx_addr as *const baseview::gl::GlContext;
                // SAFETY: see constructor comment above.
                unsafe { &*ctx }.get_proc_address(name)
            }),
        }
    }
}

unsafe impl slint::platform::femtovg_renderer::OpenGLInterface for BaseviewOpenGLInterface {
    fn ensure_current(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // baseview makes the context current before calling on_frame
        Ok(())
    }

    fn swap_buffers(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // baseview handles buffer swapping (we call swap_buffers manually at end of frame)
        Ok(())
    }

    fn resize(
        &self,
        _width: core::num::NonZeroU32,
        _height: core::num::NonZeroU32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Resize is handled via WindowAdapter::size()
        Ok(())
    }

    fn get_proc_address(&self, name: &core::ffi::CStr) -> *const core::ffi::c_void {
        let name = name.to_str().unwrap_or("");
        (self.get_proc_address)(name)
    }
}

// Hand-off slot for the adapter being constructed.
//
// Slint has a single process-global `Platform`, and the only way it hands us the
// window is by calling `Platform::create_window_adapter()` while a component is
// being constructed. We can't pass the adapter in directly, so we stash it here
// just before constructing the component and read it back from inside
// `create_window_adapter`.
//
// This is a *transient* hand-off, not shared live state: it's set and read
// synchronously on the same thread within a single `open` call, and the
// constructed component holds its own adapter reference afterward. Crucially we
// clear it again as soon as construction finishes (see `CurrentAdapterGuard`),
// so a later, stray `create_window_adapter` call can never pick up a stale
// adapter from a different window — it gets a clean "no adapter set" error
// instead. (On Windows/macOS all windows share the main thread, so without the
// clear, a shared slot would be a footgun across plugin instances.)
thread_local! {
    static CURRENT_ADAPTER: RefCell<Option<Rc<BaseviewSlintAdapter>>> = const { RefCell::new(None) };
}

/// RAII guard that publishes an adapter into [`CURRENT_ADAPTER`] for the
/// duration of component construction and clears it on drop, so the slot is only
/// ever readable during the synchronous construction it's meant for.
struct CurrentAdapterGuard;

impl CurrentAdapterGuard {
    fn set(adapter: Rc<BaseviewSlintAdapter>) -> Self {
        CURRENT_ADAPTER.with(|current| {
            *current.borrow_mut() = Some(adapter);
        });
        Self
    }
}

impl Drop for CurrentAdapterGuard {
    fn drop(&mut self) {
        CURRENT_ADAPTER.with(|current| {
            *current.borrow_mut() = None;
        });
    }
}

/// Platform implementation for Slint, backed by the current baseview adapter.
struct BaseviewSlintPlatform;

impl slint::platform::Platform for BaseviewSlintPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        CURRENT_ADAPTER.with(|adapter| {
            adapter
                .borrow()
                .clone()
                .map(|a| a as Rc<dyn WindowAdapter>)
                .ok_or_else(|| slint::PlatformError::Other("No adapter set".into()))
        })
    }
}

/// Custom `WindowAdapter` that bridges baseview and Slint.
struct BaseviewSlintAdapter {
    window: slint::Window,
    renderer: OnceCell<FemtoVGRenderer>,
    /// Physical size in actual pixels (for the OpenGL framebuffer)
    physical_size: RefCell<PhysicalSize>,
    /// Scale factor (e.g., 2.0 on Retina displays)
    scale_factor: RefCell<f32>,
    /// Stored proc-address loader, set once the GL context is available
    gl_interface: OnceCell<BaseviewOpenGLInterface>,
}

impl BaseviewSlintAdapter {
    fn new(physical_width: u32, physical_height: u32, scale_factor: f32) -> Rc<Self> {
        Rc::new_cyclic(|weak_self| {
            let window = slint::Window::new(weak_self.clone() as _);
            Self {
                window,
                renderer: OnceCell::new(),
                physical_size: RefCell::new(PhysicalSize::new(physical_width, physical_height)),
                scale_factor: RefCell::new(scale_factor),
                gl_interface: OnceCell::new(),
            }
        })
    }

    /// Call once after the GL context is live to wire up the proc-address loader.
    fn set_gl_context(&self, window: &baseview::Window) {
        let _ = self.gl_interface.set(BaseviewOpenGLInterface::new(window));
    }

    /// Update the size and scale factor (called when window is resized or scale changes)
    fn update_size(&self, physical_width: u32, physical_height: u32, scale_factor: f32) {
        *self.physical_size.borrow_mut() = PhysicalSize::new(physical_width, physical_height);
        *self.scale_factor.borrow_mut() = scale_factor;
    }
}

impl WindowAdapter for BaseviewSlintAdapter {
    fn window(&self) -> &slint::Window {
        &self.window
    }

    fn size(&self) -> PhysicalSize {
        *self.physical_size.borrow()
    }

    fn renderer(&self) -> &dyn slint::platform::Renderer {
        self.renderer.get_or_init(|| {
            let interface = self
                .gl_interface
                .get()
                .expect("GL context must be set via set_gl_context() before renderer() is called");
            FemtoVGRenderer::new(interface.clone()).expect("Failed to create FemtoVG renderer")
        })
    }

    fn request_redraw(&self) {
        // baseview handles redraws in on_frame
    }
}

/// Per-window state, passed to the `setup` and `update` closures each frame.
///
/// Generic over the Slint component type `T`. Gives access to the component, the
/// Slint window, the current scale factor, and resize control.
pub struct SlintWindowHandler<T: slint::ComponentHandle> {
    setup_handler: Arc<SetupHandler<T>>,
    update_handler: Arc<UpdateHandler<T>>,
    /// Invoked from [`resize`](Self::resize) after the size is applied, so a host
    /// integration can react (e.g. ask the host to resize its window). Receives
    /// the new logical `(width, height)`.
    on_resize: Arc<ResizeHandler>,
    scale_factor: RefCell<f32>,
    // Rc so it can be shared with Slint callbacks without needing &mut self
    pending_resizes: Rc<RefCell<Vec<(u32, u32)>>>,
    last_cursor_pos: RefCell<LogicalPosition>,
    window_shown: RefCell<bool>,
    component: T,
    adapter: Rc<BaseviewSlintAdapter>,
    prevent_key_event_propagation: RefCell<bool>,
}

impl<T: slint::ComponentHandle> SlintWindowHandler<T> {
    /// Resize the window. `width` and `height` are in logical pixels.
    ///
    /// Applies the new size to Slint and to the baseview window, then invokes the
    /// `on_resize` callback (so an integration can notify its host).
    pub fn resize(&self, window: &mut baseview::Window, width: u32, height: u32) {
        let scale = *self.scale_factor.borrow();
        let physical_width = (width as f32 * scale) as u32;
        let physical_height = (height as f32 * scale) as u32;

        // Update adapter with physical size and scale factor
        self.adapter
            .update_size(physical_width, physical_height, scale);

        // Notify Slint window of new size to trigger re-layout (logical size)
        let slint_window = self.window();
        slint_window.dispatch_event(slint::platform::WindowEvent::Resized {
            size: slint::LogicalSize::new(width as f32, height as f32),
        });

        // Request redraw to show changes immediately
        slint_window.request_redraw();

        // Let the integration react (e.g. request a host resize).
        (self.on_resize)(width, height);

        // Resize baseview window (uses logical size, baseview handles physical conversion)
        window.resize(Size {
            width: width as f64,
            height: height as f64,
        });
    }

    /// Handle a window info update from baseview (scale factor or size change).
    fn handle_window_info(&self, info: &WindowInfo) {
        let scale = info.scale() as f32;
        let physical_size = info.physical_size();

        *self.scale_factor.borrow_mut() = scale;

        // Update adapter with physical size
        self.adapter
            .update_size(physical_size.width, physical_size.height, scale);

        // Notify Slint of the new size (logical)
        let logical_size = info.logical_size();
        self.adapter
            .window
            .dispatch_event(slint::platform::WindowEvent::Resized {
                size: slint::LogicalSize::new(
                    logical_size.width as f32,
                    logical_size.height as f32,
                ),
            });

        // Also set the scale factor on the Slint window
        self.adapter
            .window
            .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
                scale_factor: scale,
            });

        // Deliberately does NOT invoke `on_resize`: this path runs for host- and scale-driven
        // window info, and an integration that calls `GuiContext::request_resize()` there would
        // ask the host to apply the size it just sent, which fights user drags and can loop.
        // Only an explicit `resize` (a plugin-initiated size change) notifies the host.
    }

    /// Queue a resize to be applied next frame. Use this from Slint callbacks
    /// where you don't have access to `&mut Window`.
    pub fn queue_resize(&self, width: u32, height: u32) {
        self.pending_resizes.borrow_mut().push((width, height));
    }

    /// Returns the resize queue so you can clone the `Rc` and push to it from
    /// callbacks.
    pub fn pending_resizes(&self) -> &Rc<RefCell<Vec<(u32, u32)>>> {
        &self.pending_resizes
    }

    /// Apply the most recent queued resize, if any. Called automatically each
    /// frame; returns the applied size.
    pub fn process_pending_resizes(&self, window: &mut baseview::Window) -> Option<(u32, u32)> {
        let mut queue = self.pending_resizes.borrow_mut();
        if let Some((width, height)) = queue.pop() {
            // Only process the most recent resize request to avoid lag
            queue.clear();
            drop(queue); // Release the borrow before calling resize

            self.resize(window, width, height);
            Some((width, height))
        } else {
            None
        }
    }

    /// The Slint component instance.
    pub fn component(&self) -> &T {
        &self.component
    }

    /// The Slint [`Window`](slint::Window).
    pub fn window(&self) -> &slint::Window {
        &self.adapter.window
    }

    /// The current scale factor (e.g. `2.0` on a HiDPI display).
    pub fn scale_factor(&self) -> f32 {
        *self.scale_factor.borrow()
    }

    /// When enabled, keyboard events are marked as captured so the host doesn't
    /// also process them. Useful while a text input in the UI has focus.
    pub fn set_prevent_key_event_propagation(&self, is_enabled: bool) {
        *self.prevent_key_event_propagation.borrow_mut() = is_enabled;
    }
}

impl<T: slint::ComponentHandle> baseview::WindowHandler for SlintWindowHandler<T> {
    fn on_frame(&mut self, window: &mut baseview::Window) {
        // Make the GL context current for this frame.
        log_backend("on_frame: entered");
        let Some(gl_context) = window.gl_context() else {
            log_backend("ERROR: baseview window has no OpenGL context");
            return;
        };
        unsafe { gl_context.make_current() };

        // On first frame: initialize the renderer and show the component.
        // We defer this until on_frame (rather than doing it in spawn's closure)
        // so the GL context is guaranteed to be current when FemtoVG queries GL_VERSION.
        if !*self.window_shown.borrow() {
            log_backend("first frame: initializing Slint renderer");
            self.adapter.set_gl_context(window);
            let _ = self.adapter.renderer.get_or_init(|| {
                self.adapter
                    .gl_interface
                    .get()
                    .map(|iface| {
                        FemtoVGRenderer::new(iface.clone())
                            .expect("Failed to create FemtoVG renderer")
                    })
                    .expect("gl_interface must be set before renderer init")
            });
            self.component.show().expect("Failed to show component");
            log_backend("first frame: component.show() succeeded");
            *self.window_shown.borrow_mut() = true;

            // Fires once, so integrations can register UI -> plugin callbacks
            // before the per-frame loop starts.
            (self.setup_handler)(self, window);
        }

        // Call the per-frame update handler first.
        (self.update_handler)(self, window);

        // Update Slint timers and animations
        slint::platform::update_timers_and_animations();

        // Process pending resizes
        self.process_pending_resizes(window);

        // Render the component - Slint handles the rendering internally; it calls
        // our WindowAdapter's renderer() method when needed.
        self.component.window().request_redraw();

        // Process Slint's internal rendering queue.
        slint::platform::duration_until_next_timer_update();

        // CRITICAL: Actually trigger the render by accessing the renderer.
        if let Some(renderer) = self.adapter.renderer.get() {
            let _ = renderer.render();
        }

        // Swap buffers after rendering. The context was checked at frame entry.
        if let Some(gl_context) = window.gl_context() {
            gl_context.swap_buffers();
        } else {
            log_backend("ERROR: OpenGL context disappeared before buffer swap");
        }
    }

    fn on_event(&mut self, window: &mut baseview::Window, event: Event) -> EventStatus {
        match event {
            Event::Mouse(mouse_event) => {
                let slint_event = match mouse_event {
                    baseview::MouseEvent::CursorMoved { position, .. } => {
                        let pos = LogicalPosition::new(position.x as f32, position.y as f32);
                        *self.last_cursor_pos.borrow_mut() = pos;
                        WindowEvent::PointerMoved { position: pos }
                    }
                    baseview::MouseEvent::ButtonPressed { button, .. } => {
                        // A parented editor window often receives the click without receiving
                        // keyboard focus from the host. Slint then shows a focused TextInput
                        // but no character keys arrive. Claiming focus here is what makes
                        // click-then-type work in embedded DAW windows.
                        window.focus();
                        let slint_button = match button {
                            baseview::MouseButton::Left => slint::platform::PointerEventButton::Left,
                            baseview::MouseButton::Right => {
                                slint::platform::PointerEventButton::Right
                            }
                            baseview::MouseButton::Middle => {
                                slint::platform::PointerEventButton::Middle
                            }
                            _ => return EventStatus::Ignored,
                        };
                        WindowEvent::PointerPressed {
                            button: slint_button,
                            position: *self.last_cursor_pos.borrow(),
                        }
                    }
                    baseview::MouseEvent::ButtonReleased { button, .. } => {
                        let slint_button = match button {
                            baseview::MouseButton::Left => slint::platform::PointerEventButton::Left,
                            baseview::MouseButton::Right => {
                                slint::platform::PointerEventButton::Right
                            }
                            baseview::MouseButton::Middle => {
                                slint::platform::PointerEventButton::Middle
                            }
                            _ => return EventStatus::Ignored,
                        };
                        WindowEvent::PointerReleased {
                            button: slint_button,
                            position: *self.last_cursor_pos.borrow(),
                        }
                    }
                    baseview::MouseEvent::WheelScrolled { delta, .. } => {
                        let (delta_x, delta_y) = match delta {
                            baseview::ScrollDelta::Lines { x, y } => (x * 20.0, y * 20.0),
                            baseview::ScrollDelta::Pixels { x, y } => (x, y),
                        };
                        // LOCAL PATCH: baseview's `WheelScrolled` carries no position, so
                        // this used to hardcode (0,0). Slint hit-tests scroll events by
                        // position, so every wheel notch was delivered to whatever sits in
                        // the top-left corner and no control's `scroll-event` ever fired.
                        // `last_cursor_pos` is already maintained from `CursorMoved` and is
                        // what the two button arms just above use.
                        WindowEvent::PointerScrolled {
                            position: *self.last_cursor_pos.borrow(),
                            delta_x,
                            delta_y,
                        }
                    }
                    // The cursor left the window: tell Slint so hover/active
                    // states clear (otherwise a control can stay "hovered" after
                    // the pointer leaves, which interferes with focus handling).
                    baseview::MouseEvent::CursorLeft => WindowEvent::PointerExited,
                    // Slint infers pointer-enter from the next `PointerMoved`, so
                    // there's nothing to dispatch here — but we capture it rather
                    // than ignore it so it isn't treated as unhandled.
                    baseview::MouseEvent::CursorEntered => return EventStatus::Captured,
                    // Drag-and-drop (DragEntered/DragMoved/DragLeft/DragDropped)
                    // isn't wired up yet; ignore for now.
                    _ => return EventStatus::Ignored,
                };

                self.adapter.window.dispatch_event(slint_event);
                EventStatus::Captured
            }
            Event::Keyboard(key_event) => {
                let text: SharedString = if let keyboard_types::Key::Character(char) = key_event.key
                {
                    char.into()
                } else {
                    // Non-character key: map it to Slint's named-key text. See
                    // `translate::key_text_from_code` for the full table.
                    match translate::key_text_from_code(key_event.code) {
                        Some(text) => text,
                        None => return EventStatus::Ignored,
                    }
                };

                if text.is_empty() {
                    return EventStatus::Ignored;
                }

                match key_event.state {
                    keyboard_types::KeyState::Down => {
                        if key_event.repeat {
                            self.adapter
                                .window
                                .dispatch_event(WindowEvent::KeyPressRepeated { text });
                        } else {
                            self.adapter
                                .window
                                .dispatch_event(WindowEvent::KeyPressed { text });
                        }
                    }
                    keyboard_types::KeyState::Up => {
                        self.adapter
                            .window
                            .dispatch_event(WindowEvent::KeyReleased { text });
                    }
                }

                if *self.prevent_key_event_propagation.borrow() {
                    EventStatus::Captured
                } else {
                    EventStatus::Ignored
                }
            }
            Event::Window(window_event) => match window_event {
                BaseviewWindowEvent::Resized(info) => {
                    self.handle_window_info(&info);
                    EventStatus::Captured
                }
                BaseviewWindowEvent::Focused => {
                    self.adapter.window.dispatch_event(WindowEvent::WindowActiveChanged(true));
                    EventStatus::Captured
                }
                BaseviewWindowEvent::Unfocused => {
                    self.adapter.window.dispatch_event(WindowEvent::WindowActiveChanged(false));
                    EventStatus::Captured
                }
                BaseviewWindowEvent::WillClose => EventStatus::Ignored,
            },
        }
    }
}

/// Entry point: open a Slint UI inside a baseview window parented to `parent`.
pub struct SlintWindow;

impl SlintWindow {
    /// Open a Slint component inside a baseview window parented to `parent`.
    ///
    /// - `component_factory` constructs the Slint component (called once on the
    ///   GUI thread, after the GL context and Slint platform are ready).
    /// - `setup` runs once after the component is shown — register UI callbacks
    ///   here.
    /// - `update` runs every frame — push parameter/state values into the UI.
    /// - `on_resize` is invoked after an explicit [`SlintWindowHandler::resize`] /
    ///   queued resize, with the new logical `(width, height)`. Integrations use it
    ///   to notify their host; pass `|_, _| {}` if not needed. Host/scale-driven
    ///   window info does not invoke it, so a host notification cannot echo back.
    ///
    /// `options.gl_config` must request an OpenGL context (see
    /// [`default_gl_config`]); the FemtoVG renderer requires it.
    #[allow(clippy::too_many_arguments)]
    pub fn open_parented<P, T, FFactory, FSetup, FUpdate, FResize>(
        parent: &P,
        options: WindowOpenOptions,
        component_factory: FFactory,
        setup: FSetup,
        update: FUpdate,
        on_resize: FResize,
    ) -> WindowHandle
    where
        P: HasRawWindowHandle,
        T: slint::ComponentHandle + 'static,
        FFactory: Fn() -> Result<T, slint::PlatformError> + Send + 'static,
        FSetup: Fn(&SlintWindowHandler<T>, &mut Window) + Send + Sync + 'static,
        FUpdate: Fn(&SlintWindowHandler<T>, &mut Window) + Send + Sync + 'static,
        FResize: Fn(u32, u32) + Send + Sync + 'static,
    {
        let build = build_handler(&options, component_factory, setup, update, on_resize);
        baseview::Window::open_parented(parent, options, build)
    }

    /// Open the Slint UI in a standalone, top-level window and **block** until it
    /// closes. Useful for examples and quick testing outside a plugin host.
    ///
    /// Same closures as [`open_parented`](Self::open_parented); there's no parent
    /// and no `WindowHandle` (the call returns when the window closes).
    #[allow(clippy::too_many_arguments)]
    pub fn open_blocking<T, FFactory, FSetup, FUpdate, FResize>(
        options: WindowOpenOptions,
        component_factory: FFactory,
        setup: FSetup,
        update: FUpdate,
        on_resize: FResize,
    ) where
        T: slint::ComponentHandle + 'static,
        FFactory: Fn() -> Result<T, slint::PlatformError> + Send + 'static,
        FSetup: Fn(&SlintWindowHandler<T>, &mut Window) + Send + Sync + 'static,
        FUpdate: Fn(&SlintWindowHandler<T>, &mut Window) + Send + Sync + 'static,
        FResize: Fn(u32, u32) + Send + Sync + 'static,
    {
        let build = build_handler(&options, component_factory, setup, update, on_resize);
        baseview::Window::open_blocking(options, build);
    }
}

/// Build the `FnOnce(&mut Window) -> SlintWindowHandler<T>` that both
/// [`SlintWindow::open_parented`] and [`SlintWindow::open_blocking`] hand to
/// baseview. Factored out so the window-setup logic lives in one place.
fn build_handler<T, FFactory, FSetup, FUpdate, FResize>(
    options: &WindowOpenOptions,
    component_factory: FFactory,
    setup: FSetup,
    update: FUpdate,
    on_resize: FResize,
) -> impl FnOnce(&mut Window) -> SlintWindowHandler<T> + Send + 'static
where
    T: slint::ComponentHandle + 'static,
    FFactory: Fn() -> Result<T, slint::PlatformError> + Send + 'static,
    FSetup: Fn(&SlintWindowHandler<T>, &mut Window) + Send + Sync + 'static,
    FUpdate: Fn(&SlintWindowHandler<T>, &mut Window) + Send + Sync + 'static,
    FResize: Fn(u32, u32) + Send + Sync + 'static,
{
    let Size { width, height } = options.size;
    let (width, height) = (width as u32, height as u32);

    let setup_handler: Arc<SetupHandler<T>> = Arc::new(setup);
    let update_handler: Arc<UpdateHandler<T>> = Arc::new(update);
    let on_resize: Arc<ResizeHandler> = Arc::new(on_resize);

    move |baseview_window: &mut Window| {
        // Make the GL context current so that any renderer creation during
        // component init (Slint may call renderer() eagerly) has a valid context.
        log_backend("window factory: entered");
        if let Some(gl_context) = baseview_window.gl_context() {
            unsafe { gl_context.make_current() };
        } else {
            log_backend("ERROR: window factory received no OpenGL context");
        }

        // Create the Slint window adapter. Start with scale 1.0; the actual
        // system scale arrives via the first WindowEvent::Resized from baseview.
        let initial_scale = 1.0f32;
        let adapter = BaseviewSlintAdapter::new(width, height, initial_scale);

        // Wire up the GL proc-address loader now that the context is live.
        adapter.set_gl_context(baseview_window);

        // Install our platform on first open; ignored (Err) on subsequent
        // opens since Slint only allows setting the platform once per process.
        if let Err(error) = slint::platform::set_platform(Box::new(BaseviewSlintPlatform)) {
            // Slint permits one process-global platform. An existing platform can be
            // expected when another instance already initialized this backend; record it
            // so a host-specific platform conflict is diagnosable.
            log_backend(&format!("set_platform returned an error: {error}"));
        } else {
            log_backend("Slint platform installed");
        }

        // Publish this window's adapter only for the duration of component
        // construction. Slint calls `create_window_adapter` synchronously while
        // building the component (below), picks up this adapter, and the
        // component then owns its reference. The guard clears the slot when this
        // scope ends, so no later call can read a stale adapter — important
        // because on Windows/macOS all plugin instances share this thread (and
        // therefore this slot).
        let component = {
            let _adapter_guard = CurrentAdapterGuard::set(adapter.clone());
            log_backend("component factory: creating Slint component");
            let result = component_factory();
            match result {
                Ok(component) => {
                    log_backend("component factory: succeeded");
                    component
                }
                Err(error) => {
                    log_backend(&format!("ERROR: Slint component creation failed: {error}"));
                    panic!("Failed to create Slint component: {}", error)
                }
            }
        };

        SlintWindowHandler {
            setup_handler,
            update_handler,
            on_resize,
            scale_factor: RefCell::new(initial_scale),
            pending_resizes: Rc::new(RefCell::new(Vec::new())),
            last_cursor_pos: RefCell::new(LogicalPosition::new(0.0, 0.0)),
            window_shown: RefCell::new(false),
            component,
            adapter,
            prevent_key_event_propagation: RefCell::new(false),
        }
    }
}