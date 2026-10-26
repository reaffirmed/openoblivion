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
//! 180 block 2            # hold the right mouse button for this many seconds
//! 200 quit
//! ```
//!
//! A step can instead name simulation time with `t` and seconds, which doesn't
//! depend on the framerate, so a recorded run replays the same way on any
//! machine. Checks make a run a test: each one that fails is logged, and the
//! run exits with an error at `quit`.
//!
//! ```text
//! t2.5 hold KeyW 1.5
//! t40 expect stage CharacterGen >= 14      # ==, >=, <=, >, <
//! t40 expect near 824 -104 0 64            # the player within 64 units
//! t40 expect actor 0002ABCD near 200 0 30 100
//! t50 expect health player >= 20           # or a reference in hex
//! t50 expect dead 0002ABCD                 # or alive
//! t60 quit
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
    ExpectStage(String, Compare, u16),
    ExpectNear(Option<u32>, Vec3, f32),
    Block(f32),
    ExpectHealth(Option<u32>, Compare, f32),
    ExpectDead(u32, bool),
}

#[derive(Debug, Clone, Copy)]
enum Compare {
    Eq,
    Ge,
    Le,
    Gt,
    Lt,
}

impl Compare {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "==" => Self::Eq,
            ">=" => Self::Ge,
            "<=" => Self::Le,
            ">" => Self::Gt,
            "<" => Self::Lt,
            _ => return None,
        })
    }

    fn holds<T: PartialOrd>(self, a: T, b: T) -> bool {
        match self {
            Self::Eq => a == b,
            Self::Ge => a >= b,
            Self::Le => a <= b,
            Self::Gt => a > b,
            Self::Lt => a < b,
        }
    }
}

/// When a step runs: on a rendered frame, or at a simulation time.
#[derive(Debug, Clone, Copy, PartialEq)]
enum When {
    Frame(u32),
    Seconds(f32),
}

#[derive(Resource)]
pub struct Script {
    steps: Vec<(When, Action)>,
    /// Steps already run.
    done: Vec<bool>,
    checks: u32,
    failed: u32,
    frame: u32,
    /// Pressed last frame, released this one.
    held_keys: Vec<KeyCode>,
    /// Held down until their seconds run out.
    holding: Vec<(KeyCode, f32)>,
    held_click: bool,
    /// Seconds the right button stays down.
    blocking: f32,
}

pub struct ScriptPlugin(pub PathBuf);

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        match load(&self.0) {
            Ok(steps) => {
                app.insert_resource(Script {
                    done: vec![false; steps.len()],
                    checks: 0,
                    failed: 0,
                    steps,
                    frame: 0,
                    held_keys: Vec::new(),
                    holding: Vec::new(),
                    held_click: false,
                    blocking: 0.0,
                })
                .add_systems(PreUpdate, run_script.after(InputSystems));
            }
            Err(e) => error!("couldn't read script {}: {e}", self.0.display()),
        }
    }
}

fn load(path: &Path) -> anyhow::Result<Vec<(When, Action)>> {
    let text = std::fs::read_to_string(path)?;
    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let bad = || anyhow::anyhow!("line {}: can't read {line:?}", n + 1);
        let when = words.next().ok_or_else(bad)?;
        let when = match when.strip_prefix('t') {
            Some(seconds) => When::Seconds(seconds.parse()?),
            None => When::Frame(when.parse()?),
        };
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
            "block" => Action::Block(number(&mut words)?),
            "expect" => match words.next().ok_or_else(bad)? {
                "stage" => {
                    let quest = words.next().ok_or_else(bad)?.to_owned();
                    let op = Compare::parse(words.next().ok_or_else(bad)?).ok_or_else(bad)?;
                    let stage = words.next().ok_or_else(bad)?.parse()?;
                    Action::ExpectStage(quest, op, stage)
                }
                "near" => {
                    let at = Vec3::new(
                        number(&mut words)?,
                        number(&mut words)?,
                        number(&mut words)?,
                    );
                    Action::ExpectNear(None, at, number(&mut words)?)
                }
                "actor" => {
                    let id = u32::from_str_radix(words.next().ok_or_else(bad)?, 16)?;
                    if words.next() != Some("near") {
                        return Err(bad());
                    }
                    let at = Vec3::new(
                        number(&mut words)?,
                        number(&mut words)?,
                        number(&mut words)?,
                    );
                    Action::ExpectNear(Some(id), at, number(&mut words)?)
                }
                "health" => {
                    let who = match words.next().ok_or_else(bad)? {
                        "player" => None,
                        id => Some(u32::from_str_radix(id, 16)?),
                    };
                    let op = Compare::parse(words.next().ok_or_else(bad)?).ok_or_else(bad)?;
                    Action::ExpectHealth(who, op, number(&mut words)?)
                }
                which @ ("dead" | "alive") => Action::ExpectDead(
                    u32::from_str_radix(words.next().ok_or_else(bad)?, 16)?,
                    which == "dead",
                ),
                _ => return Err(bad()),
            },
            _ => return Err(bad()),
        };
        steps.push((when, action));
    }
    Ok(steps)
}

fn number<'a>(words: &mut impl Iterator<Item = &'a str>) -> anyhow::Result<f32> {
    Ok(words
        .next()
        .ok_or_else(|| anyhow::anyhow!("a number is missing"))?
        .parse()?)
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
    fixed: Res<Time<Fixed>>,
    player: Query<&Transform, With<oo_sim::Player>>,
    actors: Query<(&oo_sim::PlacedRef, &Transform), Without<oo_sim::Player>>,
    world: Option<Res<oo_sim::cell::WorldData>>,
    state: Res<oo_sim::script::ScriptState>,
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
    if script.blocking > 0.0 {
        script.blocking -= dt;
        if script.blocking <= 0.0 {
            mouse.release(MouseButton::Right);
        }
    }
    let frame = script.frame;
    let now = fixed.elapsed_secs();
    let mut due: Vec<Action> = Vec::new();
    for i in 0..script.steps.len() {
        let ready = match script.steps[i].0 {
            When::Frame(f) => f == frame,
            When::Seconds(t) => !script.done[i] && now >= t,
        };
        if ready {
            script.done[i] = true;
            due.push(script.steps[i].1.clone());
        }
    }
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
                if script.failed > 0 {
                    error!(
                        "script: {} of {} checks failed",
                        script.failed, script.checks
                    );
                    exit.write(AppExit::error());
                } else {
                    info!("script: all {} checks passed", script.checks);
                    exit.write(AppExit::Success);
                }
            }
            Action::ExpectStage(quest, op, want) => {
                let id = world.as_ref().and_then(|w| {
                    w.scripting
                        .dialogue
                        .quests
                        .iter()
                        .find(|(_, q)| q.editor_id.eq_ignore_ascii_case(&quest))
                        .map(|(&id, _)| id)
                });
                let have = id.map(|id| state.stage(id));
                let pass = have.is_some_and(|h| op.holds(h, want));
                check(&mut script, pass, || {
                    format!("stage {quest} is {have:?}, wanted {op:?} {want}")
                });
            }
            Action::Block(seconds) => {
                mouse.press(MouseButton::Right);
                script.blocking = seconds;
            }
            Action::ExpectHealth(who, op, want) => {
                let r = who.unwrap_or(oo_sim::script::PLAYER_REF);
                let have = state.vitals.get(&r).map(|v| v.health);
                let pass = have.is_some_and(|h| op.holds(h, want));
                check(&mut script, pass, || {
                    format!("{r:08X} has health {have:?}, wanted {op:?} {want}")
                });
            }
            Action::ExpectDead(r, dead) => {
                let have = state.vitals.get(&r).map(|v| v.dead);
                check(&mut script, have == Some(dead), || {
                    format!("{r:08X} dead is {have:?}, wanted {dead}")
                });
            }
            Action::ExpectNear(who, at, radius) => {
                let position = match who {
                    None => player.single().ok().map(|t| t.translation),
                    Some(id) => actors
                        .iter()
                        .find(|(r, _)| r.0 == id)
                        .map(|(_, t)| t.translation),
                };
                let pass = position.is_some_and(|p| p.distance(at) <= radius);
                check(&mut script, pass, || {
                    format!("{who:08X?} is at {position:?}, wanted within {radius} of {at}")
                });
            }
        }
    }
}

fn check(script: &mut Script, pass: bool, explain: impl FnOnce() -> String) {
    script.checks += 1;
    if pass {
        info!("script check passed");
    } else {
        script.failed += 1;
        error!("script check failed: {}", explain());
    }
}
