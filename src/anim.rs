//! Loader for `assets/player_anims.bin`, the player's real animations as baked
//! by `tools/bake_anims.py`: per frame, model-space joint positions plus axes
//! for the joints whose orientation matters.

use std::collections::HashMap;
use std::fs;
use std::io;

use bevy::prelude::*;

pub const PATH: &str = "assets/player_anims.bin";

pub struct Clip {
    /// Frames to cross-fade over when this clip starts, as authored.
    pub blend: f32,
    pub frames: usize,
    data: Vec<f32>,
}

#[derive(Resource)]
pub struct Clips {
    joints: Vec<String>,
    oriented: Vec<String>,
    /// Floats per frame.
    pub stride: usize,
    clips: HashMap<String, Clip>,
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn bytes(&mut self, n: usize) -> io::Result<&[u8]> {
        let slice = self
            .data
            .get(self.at..self.at + n)
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "truncated animation file"))?;
        self.at += n;
        Ok(slice)
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn name(&mut self) -> io::Result<String> {
        let len = self.bytes(1)?[0] as usize;
        Ok(String::from_utf8_lossy(self.bytes(len)?).into_owned())
    }
}

impl Clips {
    pub fn load() -> io::Result<Self> {
        let data = fs::read(PATH)?;
        let mut c = Cursor { data: &data, at: 0 };
        if c.bytes(4)? != b"ERAN" {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a baked animation file"));
        }
        let (joint_count, oriented_count, clip_count) = (c.u32()?, c.u32()?, c.u32()?);
        let joints = (0..joint_count).map(|_| c.name()).collect::<io::Result<Vec<_>>>()?;
        let oriented = (0..oriented_count).map(|_| c.name()).collect::<io::Result<Vec<_>>>()?;
        let stride = joints.len() * 3 + oriented.len() * 9;
        let mut clips = HashMap::new();
        for _ in 0..clip_count {
            let name = c.name()?;
            let blend = c.f32()?;
            let frames = c.u32()? as usize;
            let raw = c.bytes(frames * stride * 4)?;
            let data = raw.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
            clips.insert(name, Clip { blend, frames, data });
        }
        Ok(Self { joints, oriented, stride, clips })
    }

    pub fn get(&self, name: &str) -> Option<&Clip> {
        self.clips.get(name)
    }

    /// Index of a joint's position within a pose.
    pub fn joint(&self, name: &str) -> usize {
        self.joints.iter().position(|j| j == name).unwrap_or_else(|| panic!("no joint {name}")) * 3
    }

    /// Index of an oriented joint's X axis within a pose; Y and Z follow.
    pub fn axes(&self, name: &str) -> usize {
        let i = self.oriented.iter().position(|j| j == name).unwrap_or_else(|| panic!("no axes for {name}"));
        self.joints.len() * 3 + i * 9
    }

    /// Writes the pose at `frame` into `out`, interpolating between samples.
    pub fn sample(&self, clip: &Clip, frame: f32, looped: bool, out: &mut Vec<f32>) {
        let last = (clip.frames - 1) as f32;
        let frame = if looped && last > 0.0 { frame.rem_euclid(last) } else { frame.clamp(0.0, last) };
        let i = (frame.floor() as usize).min(clip.frames - 1);
        let j = (i + 1).min(clip.frames - 1);
        let t = frame - i as f32;
        let (a, b) = (&clip.data[i * self.stride..][..self.stride], &clip.data[j * self.stride..][..self.stride]);
        out.clear();
        out.extend(a.iter().zip(b).map(|(a, b)| a + (b - a) * t));
    }
}

pub fn vec3(pose: &[f32], at: usize) -> Vec3 {
    Vec3::new(pose[at], pose[at + 1], pose[at + 2])
}
