//! `--script`: scripted input for testing without a person at the keyboard.
//!
//! Key presses, cursor moves, clicks and wheel steps go straight into Bevy's
//! input state on the frames the script names, so the window needn't have the
//! operating system's focus. One step per line, `#` starts a comment:
//!
//! ```text
//! 120 key Space          # press and release a KeyCode by name
//! 130 hold KeyW 1.5      # hold a key down for this many seconds
//! 140 where              # log the player's position
//! 150 shot out/menu.png  # save a screenshot
//! 160 move 400 300       # put the cursor here, in window pixels
//! 161 click              # left button down and up
//! 170 wheel -2           # scroll, in lines (negative is down)
//! 200 quit
//! ```

use std::path::{Path, PathBuf};

use bevy::input::InputSystems;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::math::DVec2;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::PrimaryWindow;

#[derive(Debug, Clone)]
enum Action {
    Key(KeyCode),
    Hold(KeyCode, f32),
    Where,
    Move(f64, f64),
    Click,
    Wheel(f32),
    Shot(PathBuf),
    Quit,
}

#[derive(Resource)]
pub struct Script {
    steps: Vec<(u32, Action)>,
    frame: u32,
    /// Pressed last frame, released this one.
    held_keys: Vec<KeyCode>,
    /// Held down until their seconds run out.
    holding: Vec<(KeyCode, f32)>,
    held_click: bool,
}

pub struct ScriptPlugin(pub PathBuf);

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        match load(&self.0) {
            Ok(steps) => {
                app.insert_resource(Script {
                    steps,
                    frame: 0,
                    held_keys: Vec::new(),
                    holding: Vec::new(),
                    held_click: false,
                })
                .add_systems(PreUpdate, run_script.after(InputSystems));
            }
            Err(e) => error!("couldn't read script {}: {e}", self.0.display()),
        }
    }
}

fn load(path: &Path) -> anyhow::Result<Vec<(u32, Action)>> {
    let text = std::fs::read_to_string(path)?;
    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let bad = || anyhow::anyhow!("line {}: can't read {line:?}", n + 1);
        let frame: u32 = words.next().ok_or_else(bad)?.parse()?;
        let action = match words.next().ok_or_else(bad)? {
            "key" => Action::Key(key_code(words.next().ok_or_else(bad)?).ok_or_else(bad)?),
            "hold" => Action::Hold(
                key_code(words.next().ok_or_else(bad)?).ok_or_else(bad)?,
                words.next().ok_or_else(bad)?.parse()?,
            ),
            "where" => Action::Where,
            "move" => Action::Move(
                words.next().ok_or_else(bad)?.parse()?,
                words.next().ok_or_else(bad)?.parse()?,
            ),
            "click" => Action::Click,
            "wheel" => Action::Wheel(words.next().ok_or_else(bad)?.parse()?),
            "shot" => Action::Shot(words.next().ok_or_else(bad)?.into()),
            "quit" => Action::Quit,
            _ => return Err(bad()),
        };
        steps.push((frame, action));
    }
    Ok(steps)
}

fn key_code(name: &str) -> Option<KeyCode> {
    Some(match name {
        "Space" => KeyCode::Space,
        "Tab" => KeyCode::Tab,
        "Escape" => KeyCode::Escape,
        "Enter" => KeyCode::Enter,
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        _ => {
            // KeyA..KeyZ
            let letter = name.strip_prefix("Key")?.chars().next()?;
            const KEYS: [KeyCode; 26] = [
                KeyCode::KeyA,
                KeyCode::KeyB,
                KeyCode::KeyC,
                KeyCode::KeyD,
                KeyCode::KeyE,
                KeyCode::KeyF,
                KeyCode::KeyG,
                KeyCode::KeyH,
                KeyCode::KeyI,
                KeyCode::KeyJ,
                KeyCode::KeyK,
                KeyCode::KeyL,
                KeyCode::KeyM,
                KeyCode::KeyN,
                KeyCode::KeyO,
                KeyCode::KeyP,
                KeyCode::KeyQ,
                KeyCode::KeyR,
                KeyCode::KeyS,
                KeyCode::KeyT,
                KeyCode::KeyU,
                KeyCode::KeyV,
                KeyCode::KeyW,
                KeyCode::KeyX,
                KeyCode::KeyY,
                KeyCode::KeyZ,
            ];
            *KEYS.get((letter.to_ascii_uppercase() as u8).checked_sub(b'A')? as usize)?
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn run_script(
    mut commands: Commands,
    mut script: ResMut<Script>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut wheel: MessageWriter<MouseWheel>,
    mut window: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
    time: Res<Time>,
    player: Query<&Transform, With<oo_sim::Player>>,
) {
    script.frame += 1;
    for key in std::mem::take(&mut script.held_keys) {
        keys.release(key);
    }
    let dt = time.delta_secs();
    script.holding.retain_mut(|(key, left)| {
        *left -= dt;
        if *left > 0.0 {
            keys.press(*key);
            true
        } else {
            keys.release(*key);
            false
        }
    });
    if std::mem::take(&mut script.held_click) {
        mouse.release(MouseButton::Left);
    }
    let frame = script.frame;
    let due: Vec<Action> = script
        .steps
        .iter()
        .filter(|(f, _)| *f == frame)
        .map(|(_, a)| a.clone())
        .collect();
    for action in due {
        info!("script frame {frame}: {action:?}");
        match action {
            Action::Key(key) => {
                keys.press(key);
                script.held_keys.push(key);
            }
            Action::Hold(key, seconds) => {
                keys.press(key);
                script.holding.push((key, seconds));
            }
            Action::Where => {
                if let Ok(t) = player.single() {
                    info!("script frame {frame}: player at {}", t.translation);
                }
            }
            Action::Move(x, y) => {
                if let Ok((_, mut w)) = window.single_mut() {
                    w.set_physical_cursor_position(Some(DVec2::new(x, y)));
                }
            }
            Action::Click => {
                mouse.press(MouseButton::Left);
                script.held_click = true;
            }
            Action::Wheel(lines) => {
                if let Ok((entity, _)) = window.single() {
                    wheel.write(MouseWheel {
                        unit: MouseScrollUnit::Line,
                        x: 0.0,
                        y: lines,
                        window: entity,
                        phase: bevy::input::touch::TouchPhase::Moved,
                    });
                }
            }
            Action::Shot(path) => {
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
            }
            Action::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}
