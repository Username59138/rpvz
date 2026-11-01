//! Dialogs in the original style: a panel built from nine `IMAGE_DIALOG_*`
//! pieces, a header ornament, text in the game fonts, an optional edit box and
//! buttons built from `IMAGE_BUTTON_*` caps and a tiled middle.
//!
//! Text colours and offsets inside dialogs are approximations for now.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::config::Config;
use crate::input::{Pointer, Typed};
use crate::library::{ImageCache, Library};
use crate::reanim::{Clock, ReanimSet, SCREEN, to_world};
use crate::text::{FontCache, PopText, TextAlign};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DialogResults>()
            // Typing lands in the edit box before Enter is read in the same frame.
            .add_systems(
                Update,
                (edit_fields, dialog_buttons)
                    .chain()
                    .after(ReanimSet::Advance),
            );
    }
}

pub const TITLE_FONT: &str = "FONT_DWARVENTODCRAFT24";
pub const BODY_FONT: &str = "FONT_DWARVENTODCRAFT18";
pub const BUTTON_FONT: &str = "FONT_DWARVENTODCRAFT18";
pub const EDIT_FONT: &str = "FONT_BRIANNETOD16";

const TITLE_COLOR: Color = Color::srgb(0.88, 0.73, 0.38);
const BODY_COLOR: Color = Color::srgb(0.88, 0.73, 0.38);
const BUTTON_COLOR: Color = Color::srgb(0.16, 0.86, 0.33);
const BUTTON_HOVER_COLOR: Color = Color::srgb(0.5, 1.0, 0.6);
const EDIT_COLOR: Color = Color::WHITE;

/// Dialogs sit above everything else.
const DIALOG_Z: f32 = 500.0;
const NAME_MAX_LEN: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    NewPlayer,
    Rename,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonRole {
    Ok,
    Cancel,
}

/// Closed dialogs, read (and drained) by whoever opened them.
#[derive(Resource, Default)]
pub struct DialogResults(pub Vec<DialogResult>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogResult {
    pub kind: DialogKind,
    pub button: ButtonRole,
    /// Edit box contents, if the dialog had one.
    pub text: Option<String>,
}

pub struct DialogSpec {
    pub kind: DialogKind,
    pub header: String,
    pub message: String,
    /// Initial edit box text; `None` for no edit box.
    pub edit: Option<String>,
    pub buttons: Vec<(ButtonRole, String)>,
    pub size: Vec2,
}

#[derive(Component)]
pub struct Dialog {
    pub kind: DialogKind,
    buttons: Vec<DialogButton>,
    edit: Option<Entity>,
}

struct DialogButton {
    rect: Rect,
    role: ButtonRole,
    label: Entity,
}

#[derive(Component)]
struct EditField {
    text: String,
    label: Entity,
    caret: Entity,
    /// Baseline-left of the text, game coordinates.
    origin: Vec2,
}

/// Everything needed to build UI from game images and fonts.
#[derive(SystemParam)]
pub struct UiAssets<'w> {
    pub cfg: Res<'w, Config>,
    pub library: Res<'w, Library>,
    pub cache: ResMut<'w, ImageCache>,
    pub fonts: ResMut<'w, FontCache>,
    pub images: ResMut<'w, Assets<Image>>,
}

impl UiAssets<'_> {
    pub fn image_size(&mut self, id: &str) -> Vec2 {
        let boxes = self.cfg.boxes;
        self.cache
            .get(id, &self.library, &mut self.images, !boxes)
            .map_or(Vec2::ZERO, |c| c.size.as_vec2())
    }

    pub fn text_width(&mut self, font: &str, text: &str) -> f32 {
        let boxes = self.cfg.boxes;
        self.fonts
            .get(font, &self.library, &mut self.images, !boxes)
            .map_or(0.0, |f| f.width(text))
    }

    /// Spawns image `id` covering `rect` (game coordinates), tiled if the rect
    /// is larger than the image.
    pub fn sprite(
        &mut self,
        commands: &mut Commands,
        id: &str,
        rect: Rect,
        z: f32,
        parent: Entity,
    ) -> Option<Entity> {
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return None;
        }
        let boxes = self.cfg.boxes;
        let cached = self
            .cache
            .get(id, &self.library, &mut self.images, !boxes)?;
        let size = rect.size();
        let sprite = match cached.handle {
            Some(image) => Sprite {
                image,
                custom_size: Some(size),
                image_mode: if size == cached.size.as_vec2() {
                    SpriteImageMode::Auto
                } else {
                    SpriteImageMode::Tiled {
                        tile_x: true,
                        tile_y: true,
                        stretch_value: 1.0,
                    }
                },
                ..default()
            },
            None => Sprite::from_color(Color::srgba(0.4, 0.6, 1.0, 0.35), size),
        };
        let pos = to_world(rect.min);
        Some(
            commands
                .spawn((
                    sprite,
                    Anchor::TOP_LEFT,
                    Transform::from_xyz(pos.x, pos.y, z),
                    ChildOf(parent),
                ))
                .id(),
        )
    }
}

/// Text entity at a game-coordinate baseline point.
fn text_at(commands: &mut Commands, text: PopText, at: Vec2, z: f32, parent: Entity) -> Entity {
    let pos = to_world(at);
    commands
        .spawn((text, Transform::from_xyz(pos.x, pos.y, z), ChildOf(parent)))
        .id()
}

/// Nine-slice panel from `IMAGE_DIALOG_*` pieces.
fn panel(commands: &mut Commands, ui: &mut UiAssets, rect: Rect, z: f32, parent: Entity) {
    let tl = ui.image_size("IMAGE_DIALOG_TOPLEFT");
    let br = ui.image_size("IMAGE_DIALOG_BOTTOMRIGHT");
    let (l, t, r, b) = (rect.min.x, rect.min.y, rect.max.x, rect.max.y);
    let (x1, y1, x2, y2) = (l + tl.x, t + tl.y, r - br.x, b - br.y);
    let pieces = [
        ("IMAGE_DIALOG_TOPLEFT", Rect::new(l, t, x1, y1)),
        ("IMAGE_DIALOG_TOPMIDDLE", Rect::new(x1, t, x2, y1)),
        ("IMAGE_DIALOG_TOPRIGHT", Rect::new(x2, t, r, y1)),
        ("IMAGE_DIALOG_CENTERLEFT", Rect::new(l, y1, x1, y2)),
        ("IMAGE_DIALOG_CENTERMIDDLE", Rect::new(x1, y1, x2, y2)),
        ("IMAGE_DIALOG_CENTERRIGHT", Rect::new(x2, y1, r, y2)),
        ("IMAGE_DIALOG_BOTTOMLEFT", Rect::new(l, y2, x1, b)),
        ("IMAGE_DIALOG_BOTTOMMIDDLE", Rect::new(x1, y2, x2, b)),
        ("IMAGE_DIALOG_BOTTOMRIGHT", Rect::new(x2, y2, r, b)),
    ];
    for (id, piece) in pieces {
        ui.sprite(commands, id, piece, z, parent);
    }
    let header = ui.image_size("IMAGE_DIALOG_HEADER");
    if header != Vec2::ZERO {
        let x = (l + r - header.x) / 2.0;
        let y = t - header.y * 0.55;
        ui.sprite(
            commands,
            "IMAGE_DIALOG_HEADER",
            Rect::from_corners(Vec2::new(x, y), Vec2::new(x, y) + header),
            z + 0.1,
            parent,
        );
    }
}

/// A button from left cap, tiled middle and right cap, with a centred label.
fn button(
    commands: &mut Commands,
    ui: &mut UiAssets,
    rect: Rect,
    label: &str,
    z: f32,
    parent: Entity,
) -> Entity {
    let left = ui.image_size("IMAGE_BUTTON_LEFT");
    let right = ui.image_size("IMAGE_BUTTON_RIGHT");
    let (l, t, r, b) = (rect.min.x, rect.min.y, rect.max.x, rect.max.y);
    ui.sprite(
        commands,
        "IMAGE_BUTTON_LEFT",
        Rect::new(l, t, l + left.x, b),
        z,
        parent,
    );
    ui.sprite(
        commands,
        "IMAGE_BUTTON_MIDDLE",
        Rect::new(l + left.x, t, r - right.x, b),
        z,
        parent,
    );
    ui.sprite(
        commands,
        "IMAGE_BUTTON_RIGHT",
        Rect::new(r - right.x, t, r, b),
        z,
        parent,
    );
    let ascent = ui
        .fonts
        .get(BUTTON_FONT, &ui.library, &mut ui.images, !ui.cfg.boxes)
        .map_or(12.0, |f| f.ascent());
    let baseline = Vec2::new((l + r) / 2.0, (t + b) / 2.0 + ascent / 2.0 - 2.0);
    text_at(
        commands,
        PopText::new(BUTTON_FONT, label, BUTTON_COLOR, TextAlign::Center),
        baseline,
        z + 0.2,
        parent,
    )
}

/// Opens a modal dialog centred on the screen.
pub fn open_dialog(commands: &mut Commands, ui: &mut UiAssets, spec: DialogSpec) -> Entity {
    let rect = Rect::from_center_size(SCREEN / 2.0, spec.size);
    let root = commands
        .spawn((Transform::default(), Visibility::default()))
        .id();
    panel(commands, ui, rect, DIALOG_Z, root);

    let mut y = rect.min.y + 52.0;
    text_at(
        commands,
        PopText::new(TITLE_FONT, spec.header, TITLE_COLOR, TextAlign::Center),
        Vec2::new(rect.center().x, y),
        DIALOG_Z + 1.0,
        root,
    );
    y += 34.0;
    text_at(
        commands,
        PopText::new(BODY_FONT, spec.message, BODY_COLOR, TextAlign::Center),
        Vec2::new(rect.center().x, y),
        DIALOG_Z + 1.0,
        root,
    );

    let mut edit = None;
    if let Some(initial) = spec.edit {
        let size = ui.image_size("IMAGE_EDITBOX");
        let size = if size == Vec2::ZERO {
            Vec2::new(240.0, 32.0)
        } else {
            size
        };
        let top = y + 16.0;
        let left = rect.center().x - size.x / 2.0;
        ui.sprite(
            commands,
            "IMAGE_EDITBOX",
            Rect::from_corners(Vec2::new(left, top), Vec2::new(left, top) + size),
            DIALOG_Z + 0.5,
            root,
        );
        let origin = Vec2::new(left + 12.0, top + size.y / 2.0 + 6.0);
        let label = text_at(
            commands,
            PopText::new(EDIT_FONT, initial.clone(), EDIT_COLOR, TextAlign::Left),
            origin,
            DIALOG_Z + 1.0,
            root,
        );
        let caret = commands
            .spawn((
                Sprite::from_color(EDIT_COLOR, Vec2::new(2.0, 18.0)),
                Anchor::BOTTOM_LEFT,
                Transform::from_xyz(0.0, 0.0, DIALOG_Z + 1.1),
                ChildOf(root),
            ))
            .id();
        let field = commands
            .spawn((
                EditField {
                    text: initial,
                    label,
                    caret,
                    origin,
                },
                ChildOf(root),
            ))
            .id();
        edit = Some(field);
    }

    // Buttons share the bottom row.
    let (bw, bh, gap) = (150.0, 40.0, 20.0);
    let count = spec.buttons.len() as f32;
    let row = count * bw + (count - 1.0).max(0.0) * gap;
    let top = rect.max.y - bh - 28.0;
    let mut buttons = Vec::new();
    for (i, (role, label)) in spec.buttons.iter().enumerate() {
        let left = rect.center().x - row / 2.0 + i as f32 * (bw + gap);
        let brect = Rect::new(left, top, left + bw, top + bh);
        let label = button(commands, ui, brect, label, DIALOG_Z + 0.5, root);
        buttons.push(DialogButton {
            rect: brect,
            role: *role,
            label,
        });
    }
    commands.entity(root).insert(Dialog {
        kind: spec.kind,
        buttons,
        edit,
    });
    root
}

fn dialog_buttons(
    mut commands: Commands,
    pointer: Res<Pointer>,
    typed: Res<Typed>,
    dialogs: Query<(Entity, &Dialog)>,
    fields: Query<&EditField>,
    mut labels: Query<&mut PopText>,
    mut results: ResMut<DialogResults>,
) {
    // Only the newest dialog is active.
    let Some((entity, dialog)) = dialogs.iter().max_by_key(|(e, _)| e.index()) else {
        return;
    };
    let hovered = pointer
        .pos
        .and_then(|p| dialog.buttons.iter().position(|b| b.rect.contains(p)));
    for (i, b) in dialog.buttons.iter().enumerate() {
        if let Ok(mut label) = labels.get_mut(b.label) {
            let color = if hovered == Some(i) {
                BUTTON_HOVER_COLOR
            } else {
                BUTTON_COLOR
            };
            if label.color != color {
                label.color = color;
            }
        }
    }
    let has = |role| dialog.buttons.iter().any(|b| b.role == role);
    let pressed = if pointer.just_pressed {
        hovered.map(|i| dialog.buttons[i].role)
    } else if typed.enter && has(ButtonRole::Ok) {
        Some(ButtonRole::Ok)
    } else if typed.escape && has(ButtonRole::Cancel) {
        Some(ButtonRole::Cancel)
    } else {
        None
    };
    if let Some(button) = pressed {
        let text = dialog
            .edit
            .and_then(|e| fields.get(e).ok())
            .map(|f| f.text.trim().to_owned());
        info!("dialog {:?}: {button:?} {text:?}", dialog.kind);
        results.0.push(DialogResult {
            kind: dialog.kind,
            button,
            text,
        });
        commands.entity(entity).despawn();
    }
}

fn edit_fields(
    typed: Res<Typed>,
    clock: Res<Clock>,
    mut ui: UiAssets,
    mut fields: Query<&mut EditField>,
    mut labels: Query<&mut PopText>,
    mut carets: Query<(&mut Transform, &mut Visibility), With<Sprite>>,
) {
    for mut field in &mut fields {
        let mut changed = false;
        for _ in 0..typed.backspace {
            changed |= field.text.pop().is_some();
        }
        for c in typed.text.chars() {
            // The fonts are Latin-1.
            if (c as u32) < 256 && !c.is_control() && field.text.chars().count() < NAME_MAX_LEN {
                field.text.push(c);
                changed = true;
            }
        }
        if changed && let Ok(mut label) = labels.get_mut(field.label) {
            label.text = field.text.clone();
        }
        let width = ui.text_width(EDIT_FONT, &field.text);
        if let Ok((mut t, mut vis)) = carets.get_mut(field.caret) {
            let p = to_world(field.origin + Vec2::new(width + 1.0, 3.0));
            t.translation.x = p.x;
            t.translation.y = p.y;
            *vis = if (clock.elapsed * 2.0) as u32 % 2 == 0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}
