//! Your look to the server (the Character painter, `players.looks`).

use dwell_models::paint::Paint;
use dwell_protocol::Message;

use super::*;

impl<S: Scene> Game<S> {
    /// Whether the server takes looks; yours goes as soon as it does.
    pub fn set_looks_synced(&mut self, on: bool) {
        self.looks.synced = on;
        self.flush_look();
    }

    /// Your paint (None: the model's own colours), drawn on your body and sent to the server.
    pub fn set_own_look(&mut self, paint: Option<Paint>) {
        self.looks.set_own(paint);
        self.flush_look();
    }

    fn flush_look(&mut self) {
        if !self.looks.synced || !self.looks.dirty {
            return;
        }
        self.looks.dirty = false;
        let look = self
            .looks
            .own()
            .map_or_else(dwell_protocol::looks::PlayerLook::default, |(_, p)| {
                crate::looks::look_of(p)
            });
        self.outbox
            .borrow_mut()
            .push(Send::Control(Message::Looks(vec![look])));
    }
}

/// What the painter does at the crosshair (the web API's `paint`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintOp {
    /// The brush with a slot and a radius (cm).
    Brush {
        slot: u8,
        radius_cm: u16,
    },
    Fill {
        slot: u8,
    },
    EndStroke,
    Undo,
    Redo,
    Reset,
}

impl<S: Scene> Game<S> {
    /// Opens the Character painter (docs/CHARACTERS.md §9.4): your paint (or the model's) on a
    /// 30 m giant on an empty stage, the camera placed in front of it to fly (theater's free
    /// camera, `set_view`). False if it was open.
    pub fn open_painter(&mut self) -> bool {
        if self.looks.painter.is_some() {
            return false;
        }
        let Some((data, model)) = dwell_models::model(dwell_models::PLAYER) else {
            return false;
        };
        let at = self.position();
        let face = dwell_world::FaceFrame::at_height(at[1]);
        let paint = self.looks.own().map_or_else(
            || dwell_models::paint::default_paint(data, model),
            |(_, p)| (**p).clone(),
        );
        let painter = crate::looks::Painter::new(paint, at, face, 0.0);
        // The camera 45 m in front of it at its middle, looking at it (along -z).
        let g = painter.giant;
        let up = face.up();
        let ahead = face.vector([0.0, 0.0, 45.0]);
        let eye: [f64; 3] = std::array::from_fn(|k| {
            g.feet[k] + up[k] * crate::looks::STAGE_HEIGHT_M * 0.5 + ahead[k]
        });
        let want = face.vector([0.0, 0.0, -1.0]);
        let yaw_deg = (0..360)
            .step_by(5)
            .map(f64::from)
            .max_by(|a, b| {
                let d = |y: f64| {
                    let l = face.look(y, 0.0);
                    l[0] * want[0] + l[1] * want[1] + l[2] * want[2]
                };
                d(*a).total_cmp(&d(*b))
            })
            .unwrap_or(0.0);
        self.set_view(Some(ViewOverride {
            eye,
            yaw_deg,
            pitch_deg: 0.0,
            fov_deg: None,
            up: Some(face),
        }));
        self.looks.painter = Some(painter);
        self.looks.refresh_painting();
        self.scene.borrow_mut().set_stage(Some(g));
        true
    }

    /// Closes the painter: its paint becomes yours (`keep`), the camera back to your eyes.
    pub fn close_painter(&mut self, keep: bool) -> bool {
        let Some(p) = self.looks.painter.take() else {
            return false;
        };
        self.looks.refresh_painting();
        if keep {
            self.set_own_look(Some(p.paint));
        }
        self.set_view(None);
        self.scene.borrow_mut().set_stage(None);
        true
    }

    /// A painter action at the crosshair (the view's look). Whether the paint changed.
    pub fn paint(&mut self, op: PaintOp) -> bool {
        let Some(v) = self.view() else {
            return false;
        };
        let dir = v.face().look(v.yaw_deg, v.pitch_deg);
        let Some(p) = &mut self.looks.painter else {
            return false;
        };
        let changed = match op {
            PaintOp::Brush { slot, radius_cm } => {
                p.brush(v.eye, dir, slot, f32::from(radius_cm) / 100.0)
            }
            PaintOp::Fill { slot } => p.fill(v.eye, dir, slot),
            PaintOp::EndStroke => {
                p.end_stroke();
                false
            }
            PaintOp::Undo => p.undo(),
            PaintOp::Redo => p.redo(),
            PaintOp::Reset => p.reset(),
        };
        if changed {
            let g = p.giant;
            self.looks.refresh_painting();
            self.scene.borrow_mut().set_stage(Some(g));
        }
        changed
    }

    /// A painter slot's colour (a paint ramp). Whether it changed.
    pub fn paint_slot(&mut self, k: usize, ramp: &str) -> bool {
        let Some(p) = &mut self.looks.painter else {
            return false;
        };
        if !p.set_slot(k, ramp) {
            return false;
        }
        let g = p.giant;
        self.looks.refresh_painting();
        self.scene.borrow_mut().set_stage(Some(g));
        true
    }

    /// The painter's slots (ramp names), None when it is closed.
    pub fn painter_slots(&self) -> Option<Vec<String>> {
        self.looks.painter.as_ref().map(|p| p.paint.slots.to_vec())
    }
}
