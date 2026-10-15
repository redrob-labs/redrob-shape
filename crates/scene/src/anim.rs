//! Keyframe animation, after Blender's: an object owns an [`Action`] made of F-curves, one per
//! transform channel (Location / Rotation / Scale X, Y, Z); a keyframe pins a value at a frame
//! and the curve interpolates between keys (Constant, Linear, or Bezier with auto-clamped handles,
//! Blender's default). The scene keeps the current frame, range and fps; evaluating writes the
//! curves into the objects' transforms.

use serde::{Deserialize, Serialize};
use shape_math::{Quat, glam_euler};

use crate::{ObjectId, Scene};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Interpolation {
    Constant,
    Linear,
    #[default]
    Bezier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Channel {
    Location(u8),
    /// Euler XYZ, radians.
    Rotation(u8),
    Scale(u8),
}

impl Channel {
    pub const ALL: [Channel; 9] = [
        Channel::Location(0),
        Channel::Location(1),
        Channel::Location(2),
        Channel::Rotation(0),
        Channel::Rotation(1),
        Channel::Rotation(2),
        Channel::Scale(0),
        Channel::Scale(1),
        Channel::Scale(2),
    ];
    pub fn name(self) -> String {
        let axis = |a: u8| ["X", "Y", "Z"][a as usize % 3];
        match self {
            Channel::Location(a) => format!("{} Location", axis(a)),
            Channel::Rotation(a) => format!("{} Euler Rotation", axis(a)),
            Channel::Scale(a) => format!("{} Scale", axis(a)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub frame: f32,
    pub value: f32,
    #[serde(default)]
    pub interpolation: Interpolation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FCurve {
    pub channel: Channel,
    /// Sorted by frame.
    pub keys: Vec<Keyframe>,
}

impl FCurve {
    pub fn new(channel: Channel) -> Self {
        Self { channel, keys: Vec::new() }
    }

    /// Insert or replace the key at `frame`.
    pub fn insert(&mut self, frame: f32, value: f32) {
        match self.keys.iter().position(|k| (k.frame - frame).abs() < 1e-3) {
            Some(i) => self.keys[i].value = value,
            None => {
                let i = self.keys.iter().position(|k| k.frame > frame).unwrap_or(self.keys.len());
                self.keys.insert(i, Keyframe { frame, value, interpolation: Interpolation::Bezier });
            }
        }
    }

    pub fn remove(&mut self, frame: f32) -> bool {
        let n = self.keys.len();
        self.keys.retain(|k| (k.frame - frame).abs() >= 1e-3);
        self.keys.len() != n
    }

    /// Value at `frame`: held flat outside the keyed range.
    pub fn evaluate(&self, frame: f32) -> Option<f32> {
        let keys = &self.keys;
        let (first, last) = (keys.first()?, keys.last()?);
        if frame <= first.frame {
            return Some(first.value);
        }
        if frame >= last.frame {
            return Some(last.value);
        }
        let i = keys.iter().rposition(|k| k.frame <= frame)?;
        let (a, b) = (keys[i], keys[i + 1]);
        let t = (frame - a.frame) / (b.frame - a.frame).max(1e-6);
        Some(match a.interpolation {
            Interpolation::Constant => a.value,
            Interpolation::Linear => a.value + (b.value - a.value) * t,
            Interpolation::Bezier => {
                // Auto-clamped handles: a key that is a local extremum (or an end) gets a flat
                // tangent, others take the slope between their neighbours.
                let slope = |i: usize| -> f32 {
                    let k = keys[i];
                    let (Some(p), Some(n)) = (i.checked_sub(1).map(|j| keys[j]), keys.get(i + 1)) else { return 0.0 };
                    if (k.value - p.value) * (n.value - k.value) <= 0.0 { 0.0 } else { (n.value - p.value) / (n.frame - p.frame).max(1e-6) }
                };
                let h = b.frame - a.frame;
                let (m0, m1) = (slope(i) * h, slope(i + 1) * h);
                let (t2, t3) = (t * t, t * t * t);
                (2.0 * t3 - 3.0 * t2 + 1.0) * a.value + (t3 - 2.0 * t2 + t) * m0 + (-2.0 * t3 + 3.0 * t2) * b.value + (t3 - t2) * m1
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Action {
    pub curves: Vec<FCurve>,
}

impl Action {
    pub fn curve_mut(&mut self, channel: Channel) -> &mut FCurve {
        if let Some(i) = self.curves.iter().position(|c| c.channel == channel) {
            &mut self.curves[i]
        } else {
            self.curves.push(FCurve::new(channel));
            self.curves.sort_by_key(|c| c.channel);
            let i = self.curves.iter().position(|c| c.channel == channel).expect("just inserted");
            &mut self.curves[i]
        }
    }

    pub fn curve(&self, channel: Channel) -> Option<&FCurve> {
        self.curves.iter().find(|c| c.channel == channel)
    }

    /// Every keyed frame, sorted and unique (the Dope Sheet summary row).
    pub fn keyed_frames(&self) -> Vec<f32> {
        let mut f: Vec<f32> = self.curves.iter().flat_map(|c| c.keys.iter().map(|k| k.frame)).collect();
        f.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        f.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
        f
    }

    pub fn is_empty(&self) -> bool {
        self.curves.iter().all(|c| c.keys.is_empty())
    }
}

/// Which channels `I` keys (Blender's Insert Keyframe menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyingSet {
    Location,
    Rotation,
    Scale,
    LocRot,
    LocRotScale,
}

impl KeyingSet {
    pub const ALL: [KeyingSet; 5] = [KeyingSet::Location, KeyingSet::Rotation, KeyingSet::Scale, KeyingSet::LocRot, KeyingSet::LocRotScale];
    pub fn name(self) -> &'static str {
        match self {
            KeyingSet::Location => "Location",
            KeyingSet::Rotation => "Rotation",
            KeyingSet::Scale => "Scale",
            KeyingSet::LocRot => "Location & Rotation",
            KeyingSet::LocRotScale => "Location, Rotation & Scale",
        }
    }
    pub fn channels(self) -> Vec<Channel> {
        Channel::ALL
            .into_iter()
            .filter(|c| {
                matches!(
                    (self, c),
                    (KeyingSet::Location, Channel::Location(_))
                        | (KeyingSet::Rotation, Channel::Rotation(_))
                        | (KeyingSet::Scale, Channel::Scale(_))
                        | (KeyingSet::LocRot, Channel::Location(_) | Channel::Rotation(_))
                        | (KeyingSet::LocRotScale, _)
                )
            })
            .collect()
    }
}

/// Scene time: current frame, range and rate. `playing` is not saved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub frame: i32,
    pub start: i32,
    pub end: i32,
    pub fps: f32,
    #[serde(skip)]
    pub playing: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Self { frame: 1, start: 1, end: 250, fps: 24.0, playing: false }
    }
}

impl Scene {
    /// Key the object's current transform on the given channels at `frame`.
    pub fn insert_keyframe(&mut self, id: ObjectId, frame: i32, set: KeyingSet) -> bool {
        let Some(o) = self.object_mut(id) else { return false };
        let t = o.transform;
        let euler = t.rotation.to_euler(glam_euler());
        let action = o.action.get_or_insert_with(Action::default);
        for ch in set.channels() {
            let value = match ch {
                Channel::Location(a) => t.translation[a as usize],
                Channel::Rotation(a) => [euler.0, euler.1, euler.2][a as usize],
                Channel::Scale(a) => t.scale[a as usize],
            };
            action.curve_mut(ch).insert(frame as f32, value);
        }
        true
    }

    /// Remove every key at `frame`; returns how many curves lost one.
    pub fn delete_keyframe(&mut self, id: ObjectId, frame: i32) -> usize {
        let Some(o) = self.object_mut(id) else { return 0 };
        let Some(action) = o.action.as_mut() else { return 0 };
        let n = action.curves.iter_mut().map(|c| usize::from(c.remove(frame as f32))).sum();
        action.curves.retain(|c| !c.keys.is_empty());
        if action.is_empty() {
            o.action = None;
        }
        n
    }

    /// Write every action's curves at `frame` into the object transforms. Unkeyed channels keep
    /// their value.
    pub fn evaluate_animation(&mut self, frame: f32) {
        for o in self.objects.iter_mut().flatten() {
            let Some(action) = &o.action else { continue };
            let mut t = o.transform;
            let mut euler = t.rotation.to_euler(glam_euler());
            let mut rotated = false;
            for c in &action.curves {
                let Some(v) = c.evaluate(frame) else { continue };
                match c.channel {
                    Channel::Location(a) => t.translation[a as usize] = v,
                    Channel::Scale(a) => t.scale[a as usize] = v,
                    Channel::Rotation(a) => {
                        rotated = true;
                        match a {
                            0 => euler.0 = v,
                            1 => euler.1 = v,
                            _ => euler.2 = v,
                        }
                    }
                }
            }
            if rotated {
                t.rotation = Quat::from_euler(glam_euler(), euler.0, euler.1, euler.2);
            }
            o.transform = t;
        }
    }

    /// Keyed frames of the object, for the timeline.
    pub fn keyed_frames(&self, id: ObjectId) -> Vec<f32> {
        self.object(id).and_then(|o| o.action.as_ref()).map(|a| a.keyed_frames()).unwrap_or_default()
    }

    /// Does any object have an action key at this frame?
    pub fn has_key_at(&self, id: ObjectId, frame: i32) -> bool {
        self.keyed_frames(id).iter().any(|f| (f - frame as f32).abs() < 1e-3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Object, ObjectData, Transform};
    use shape_math::Vec3;

    #[test]
    fn interpolation_modes() {
        let mut c = FCurve::new(Channel::Location(0));
        c.insert(1.0, 0.0);
        c.insert(11.0, 10.0);
        assert_eq!(c.evaluate(0.0), Some(0.0), "held before the first key");
        assert_eq!(c.evaluate(20.0), Some(10.0), "held after the last");
        // Bezier between two keys eases in and out: slower than linear at the start, same middle.
        assert!((c.evaluate(6.0).unwrap() - 5.0).abs() < 1e-4);
        assert!(c.evaluate(2.0).unwrap() < 1.0 && c.evaluate(2.0).unwrap() > 0.0);
        c.keys[0].interpolation = Interpolation::Linear;
        assert!((c.evaluate(2.0).unwrap() - 1.0).abs() < 1e-5);
        c.keys[0].interpolation = Interpolation::Constant;
        assert_eq!(c.evaluate(10.9), Some(0.0));
        // Replacing a key keeps the count; inserting out of order sorts.
        c.insert(11.0, 20.0);
        c.insert(5.0, 3.0);
        assert_eq!(c.keys.iter().map(|k| k.frame as i32).collect::<Vec<_>>(), vec![1, 5, 11]);
        assert_eq!(c.keys[2].value, 20.0);
    }

    #[test]
    fn bezier_is_monotone_through_three_increasing_keys_and_flat_at_an_extremum() {
        let mut c = FCurve::new(Channel::Location(0));
        c.insert(0.0, 0.0);
        c.insert(10.0, 5.0);
        c.insert(20.0, 10.0);
        let mut prev = -1.0;
        for f in 0..=40 {
            let v = c.evaluate(f as f32 * 0.5).unwrap();
            assert!(v >= prev - 1e-5, "monotone: {v} after {prev}");
            prev = v;
        }
        // A peak key: values just around it do not overshoot.
        let mut p = FCurve::new(Channel::Location(0));
        p.insert(0.0, 0.0);
        p.insert(10.0, 5.0);
        p.insert(20.0, 0.0);
        assert!(p.evaluate(9.0).unwrap() <= 5.0 + 1e-5 && p.evaluate(11.0).unwrap() <= 5.0 + 1e-5);
    }

    #[test]
    fn keyframes_drive_the_object_transform() {
        let mut s = Scene::new("a");
        let id = s.add_object(Object::new("Empty", ObjectData::Empty));
        s.object_mut(id).unwrap().transform = Transform::from_translation(Vec3::new(0.0, 0.0, 0.0));
        assert!(s.insert_keyframe(id, 1, KeyingSet::Location));
        s.object_mut(id).unwrap().transform = Transform::from_translation(Vec3::new(4.0, 0.0, 0.0));
        s.insert_keyframe(id, 21, KeyingSet::LocRotScale);
        assert_eq!(s.keyed_frames(id), vec![1.0, 21.0]);
        s.evaluate_animation(11.0);
        let x = s.object(id).unwrap().transform.translation.x;
        assert!((x - 2.0).abs() < 1e-4, "midpoint {x}");
        s.evaluate_animation(1.0);
        assert_eq!(s.object(id).unwrap().transform.translation.x, 0.0);
        assert_eq!(s.delete_keyframe(id, 21), 9);
        assert_eq!(s.keyed_frames(id), vec![1.0]);
        assert_eq!(s.delete_keyframe(id, 1), 3);
        assert!(s.object(id).unwrap().action.is_none(), "an empty action is dropped");
    }
}
