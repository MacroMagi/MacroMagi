//! Checks against the real machine. These are ignored by default because they
//! need a desktop session (and, on macOS, permissions), which CI does not have.
//!
//! Run them with:
//!
//! ```text
//! cargo test -p magi-platform --test system -- --ignored --test-threads=1
//! ```
//!
//! They are written to be harmless: the pointer is "moved" by zero pixels, and
//! the captured screen is only measured, never stored.

use std::time::{Duration, Instant};

use magi_core::host::Host;
use magi_core::input::InputEvent;
use magi_platform::{InputHub, SystemHost, capture};

#[test]
#[ignore = "needs a desktop session"]
fn the_pointer_can_be_read_and_moved() {
    magi_platform::init();
    let host = SystemHost::new();
    let before = host.cursor().expect("reading the pointer position");
    host.send(&InputEvent::MoveBy { dx: 0, dy: 0 }).expect("sending a zero-distance move");
    host.send(&InputEvent::MoveTo { x: before.x, y: before.y }).expect("moving the pointer to where it already is");
    let after = host.cursor().expect("reading the pointer position again");
    assert_eq!(before, after, "an absolute move must land exactly on the requested pixel");
}

#[test]
#[ignore = "needs a desktop session"]
fn the_screen_can_be_captured() {
    magi_platform::init();
    let monitors = capture::monitors().expect("listing monitors");
    let primary = monitors.first().expect("at least one monitor");
    assert!(primary.rect.w > 0 && primary.rect.h > 0);

    let whole = capture::capture(None).expect("capturing the primary monitor");
    assert_eq!((whole.width, whole.height), (primary.rect.w, primary.rect.h));
    assert_eq!(whole.rgba.len(), whole.width as usize * whole.height as usize * 4);

    // A region comes back at exactly the size and place asked for.
    let mut region = primary.rect;
    (region.x, region.y, region.w, region.h) = (region.x + 10, region.y + 20, 64, 48);
    let part = capture::capture(Some(region)).expect("capturing a region");
    assert_eq!((part.origin.x, part.origin.y, part.width, part.height), (region.x, region.y, 64, 48));
}

#[test]
#[ignore = "needs a desktop session"]
fn the_listener_sees_input_and_recognises_our_own() {
    magi_platform::init();
    let hub = InputHub::start();
    let events = hub.subscribe();
    std::thread::sleep(Duration::from_millis(300)); // let the hooks install
    assert_eq!(hub.listener_error(), None);

    let host = SystemHost::new();
    let here = host.cursor().expect("reading the pointer position");
    host.send(&InputEvent::MoveTo { x: here.x, y: here.y }).expect("sending a move");

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut seen_own = false;
    while Instant::now() < deadline && !seen_own {
        if let Ok(observed) = events.recv_timeout(Duration::from_millis(100)) {
            let is_our_move = matches!(observed.event, InputEvent::MoveTo { .. }) && observed.synthetic;
            seen_own |= is_our_move;
        }
    }
    if cfg!(windows) {
        assert!(seen_own, "the hook should report our injected move, marked as synthetic");
    }
}

/// Presses keys the way a person would (no MacroMagi marker) and checks the
/// hook reports them. F13 and F14 have no default action anywhere, so this is
/// harmless to whatever has focus.
#[test]
#[ignore = "needs a desktop session"]
fn the_listener_reports_real_key_presses() {
    use enigo::{Direction, Enigo, Keyboard, Settings};
    use magi_core::input::Key;

    magi_platform::init();
    let hub = InputHub::start();
    let events = hub.subscribe();
    std::thread::sleep(Duration::from_millis(300));

    let mut user = Enigo::new(&Settings::default()).expect("connecting to the input system");
    let f13 = enigo::Key::Other(0x7C);
    let ctrl = enigo::Key::Other(0xA2);
    user.key(ctrl, Direction::Press).unwrap();
    user.key(f13, Direction::Press).unwrap();
    user.key(f13, Direction::Release).unwrap();
    user.key(ctrl, Direction::Release).unwrap();

    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && seen.len() < 4 {
        // Only the keys this test pressed; the person at the keyboard may be typing too.
        if let Ok(observed) = events.recv_timeout(Duration::from_millis(50))
            && matches!(
                observed.event.button_edge(),
                Some((magi_core::input::Button::Key(Key::F13 | Key::ControlLeft), _))
            )
        {
            assert!(!observed.synthetic, "input without our marker is the user's");
            seen.push(observed.event);
        }
    }
    assert_eq!(
        seen,
        vec![
            InputEvent::KeyDown { key: Key::ControlLeft },
            InputEvent::KeyDown { key: Key::F13 },
            InputEvent::KeyUp { key: Key::F13 },
            InputEvent::KeyUp { key: Key::ControlLeft },
        ]
    );
}

/// Raw Input reports relative motion, which smart recordings use for games.
#[test]
#[ignore = "needs a desktop session"]
fn relative_motion_is_reported() {
    magi_platform::init();
    let hub = InputHub::start();
    let events = hub.subscribe();
    std::thread::sleep(Duration::from_millis(300));
    let host = SystemHost::new();
    // There and back: the pointer ends where it was.
    host.send(&InputEvent::MoveBy { dx: 1, dy: 0 }).unwrap();
    host.send(&InputEvent::MoveBy { dx: -1, dy: 0 }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut relative = 0;
    while Instant::now() < deadline && relative < 2 {
        if let Ok(observed) = events.recv_timeout(Duration::from_millis(50))
            && matches!(observed.event, InputEvent::MoveBy { .. })
        {
            relative += 1;
        }
    }
    assert_eq!(relative, 2, "both relative moves are reported");
}

#[test]
#[ignore = "needs a desktop session"]
fn the_program_in_front_and_running_programs_are_known() {
    let foreground = magi_platform::system::foreground_process();
    assert!(foreground.as_deref().is_some_and(|name| !name.is_empty()), "{foreground:?}");
    let processes = magi_platform::system::running_processes();
    assert!(processes.len() > 5);
    assert!(processes.iter().any(|p| p.has_window));
}
