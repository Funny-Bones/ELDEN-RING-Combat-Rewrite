//! The player's sounds, as baked by `tools/bake_sounds.py`: per clip, the
//! frames its sound events fire on, and per sound, the game's choice of what
//! to play (one of several recordings, several at once), each at the level and
//! pitch its Wwise mix gives it, varied a little every time as the game does.
//! The recordings are `assets/sounds/<id>.ogg`.

use std::collections::HashMap;
use std::fs;
use std::io;

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::rig::Rig;

pub const PATH: &str = "assets/player_sounds.bin";

/// Overall level on top of each sound's own mix.
const MASTER_DB: f32 = 0.0;

/// One recording and how it plays: a fixed level and pitch, plus a random
/// offset within a range drawn each time.
#[derive(Clone, Copy, Debug)]
struct Recording {
    id: u32,
    db: f32,
    db_range: (f32, f32),
    cents: f32,
    cents_range: (f32, f32),
}

enum Tree {
    Media(Recording),
    Random(Vec<Tree>),
    All(Vec<Tree>),
}

#[derive(Resource)]
pub struct Sounds {
    trees: Vec<Tree>,
    /// Per clip: (frame, sound) pairs, in frame order.
    clips: HashMap<String, Vec<(f32, usize)>>,
    handles: HashMap<u32, Handle<AudioSource>>,
    /// Clip and frame heard last update.
    last: Option<(String, f32)>,
    rng: u32,
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
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "truncated sound file"))?;
        self.at += n;
        Ok(slice)
    }

    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn tree(&mut self) -> io::Result<Tree> {
        Ok(match self.u8()? {
            0 => Tree::Media(Recording {
                id: self.u32()?,
                db: self.f32()?,
                db_range: (self.f32()?, self.f32()?),
                cents: self.f32()?,
                cents_range: (self.f32()?, self.f32()?),
            }),
            kind => {
                let n = self.u16()?;
                let children = (0..n).map(|_| self.tree()).collect::<io::Result<Vec<_>>>()?;
                if kind == 1 {
                    Tree::Random(children)
                } else {
                    Tree::All(children)
                }
            }
        })
    }
}

impl Sounds {
    pub fn load() -> io::Result<Self> {
        let data = fs::read(PATH)?;
        let mut c = Cursor { data: &data, at: 0 };
        if c.bytes(4)? != b"ERSD" {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a baked sound file"));
        }
        let version = c.u32()?;
        if version != 2 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("sound file version {version}, expected 2: bake it again")));
        }
        let trees = (0..c.u32()?).map(|_| c.tree()).collect::<io::Result<Vec<_>>>()?;
        let mut clips = HashMap::new();
        for _ in 0..c.u32()? {
            let len = c.u8()? as usize;
            let name = String::from_utf8_lossy(c.bytes(len)?).into_owned();
            let events = (0..c.u16()?)
                .map(|_| Ok((c.f32()?, c.u32()? as usize)))
                .collect::<io::Result<Vec<_>>>()?;
            clips.insert(name, events);
        }
        Ok(Self { trees, clips, handles: HashMap::new(), last: None, rng: 0x9E37_79B9 })
    }
}

/// xorshift: plenty to pick one of a handful of recordings.
fn next(rng: &mut u32) -> u32 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 17;
    *rng ^= *rng << 5;
    *rng
}

fn random(rng: &mut u32, n: usize) -> usize {
    next(rng) as usize % n
}

/// Uniform in `range`.
fn within(rng: &mut u32, (low, high): (f32, f32)) -> f32 {
    low + (high - low) * (next(rng) as f32 / u32::MAX as f32)
}

/// The recordings a sound plays this time: (id, dB, playback speed).
fn pick(tree: &Tree, rng: &mut u32, out: &mut Vec<(u32, f32, f32)>) {
    match tree {
        Tree::Media(r) => {
            let db = r.db + within(rng, r.db_range);
            let cents = r.cents + within(rng, r.cents_range);
            // Wwise pitch is in cents: 1200 to the octave, and playing faster raises it.
            out.push((r.id, db, 2f32.powf(cents / 1200.0)));
        }
        Tree::All(children) => children.iter().for_each(|child| pick(child, rng, out)),
        Tree::Random(children) => {
            let i = random(rng, children.len());
            pick(&children[i], rng, out);
        }
    }
}

/// The frame of a clip the sound system should hear. Loops driven by the wall
/// clock (idle, airborne) count frames up for ever; they are wrapped to the
/// clip's length, or their sounds would only fire on the first pass.
pub fn heard_frame(frame: f32, looped: bool, clip_frames: usize) -> f32 {
    let last = (clip_frames.max(2) - 1) as f32;
    if looped {
        frame.rem_euclid(last)
    } else {
        frame
    }
}

/// Sounds of `events` the animation passed through between `last` (the frame
/// heard last update, if it was this same clip) and `frame`. A looped clip
/// that wrapped round fires the end of the loop and its start; a new clip
/// fires whatever it starts on.
fn due(events: &[(f32, usize)], last: Option<f32>, frame: f32, looped: bool) -> Vec<usize> {
    let windows: &[(f32, f32)] = &match last {
        Some(last) if frame >= last => [(last, frame), (0.0, -1.0)],
        Some(last) if looped => [(last, f32::INFINITY), (-1.0, frame)],
        // The same action started over, or a new clip.
        Some(_) => [(-1.0, frame), (0.0, -1.0)],
        None => [(frame - 1.0, frame), (0.0, -1.0)],
    };
    events
        .iter()
        .filter(|(f, _)| windows.iter().any(|&(from, to)| *f > from && *f <= to))
        .map(|&(_, sound)| sound)
        .collect()
}

/// Plays the sound events the drawn animation passed through since last update.
pub fn play(mut commands: Commands, sounds: Option<ResMut<Sounds>>, rig: Res<Rig>, assets: Res<AssetServer>) {
    let Some(mut sounds) = sounds else {
        return;
    };
    let (clip, frame) = (rig.clip.clone(), rig.frame);
    let last = sounds.last.replace((clip.clone(), frame));
    let Some(events) = sounds.clips.get(&clip) else {
        return;
    };
    let fire = due(events, last.as_ref().filter(|(c, _)| *c == clip).map(|&(_, f)| f), frame, rig.looped);
    let Sounds { trees, handles, rng, .. } = &mut *sounds;
    let mut recordings = Vec::new();
    for sound in fire {
        pick(&trees[sound], rng, &mut recordings);
    }
    if !recordings.is_empty() {
        debug!("{clip} frame {frame:.1}: playing {recordings:?}");
    }
    for (id, db, speed) in recordings {
        let handle = handles.entry(id).or_insert_with(|| assets.load(format!("sounds/{id}.ogg"))).clone();
        commands.spawn((
            AudioPlayer::new(handle),
            PlaybackSettings::DESPAWN.with_volume(Volume::Decibels(db + MASTER_DB)).with_speed(speed),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::audio::Decodable;

    #[test]
    fn events_fire_once_as_frames_pass() {
        let events = [(0.0, 0), (5.0, 1), (15.0, 2)];
        assert_eq!(due(&events, None, 0.3, false), vec![0]);
        assert_eq!(due(&events, Some(0.3), 4.9, false), Vec::<usize>::new());
        assert_eq!(due(&events, Some(4.9), 5.0, false), vec![1]);
        assert_eq!(due(&events, Some(5.0), 5.5, false), Vec::<usize>::new());
        // A loop wrapping from 14 to 2 passes 15, then 0 again.
        assert_eq!(due(&events, Some(14.0), 2.0, true), vec![0, 2]);
        // An action restarting from its beginning.
        assert_eq!(due(&events, Some(20.0), 0.5, false), vec![0]);
    }

    /// A loop on the wall clock keeps firing its sounds, once per pass.
    #[test]
    fn wall_clock_loops_repeat_their_sounds() {
        let events = [(10.0, 0)];
        let clip_frames = 91; // a 90-frame loop, like the idle
        let mut fired = 0;
        let mut last = None;
        // Five passes of the loop, at an awkward step so frames never land exactly.
        let mut clock = 0.0_f32;
        while clock < 450.0 {
            let frame = heard_frame(clock, true, clip_frames);
            fired += due(&events, last, frame, true).len();
            last = Some(frame);
            clock += 0.7;
        }
        assert_eq!(fired, 5);
        // A one-shot clip is passed through untouched.
        assert_eq!(heard_frame(123.4, false, clip_frames), 123.4);
    }

    /// The baked recordings decode with the decoder Bevy plays them through.
    #[test]
    fn baked_recordings_decode() {
        let Ok(dir) = fs::read_dir("assets/sounds") else {
            eprintln!("no assets/sounds; bake them with tools/bake_sounds.py");
            return;
        };
        let mut checked = 0;
        for entry in dir.take(25) {
            let path = entry.unwrap().path();
            let source = AudioSource { bytes: fs::read(&path).unwrap().into() };
            let samples = source.decoder().count();
            assert!(samples > 100, "{} decoded to {samples} samples", path.display());
            checked += 1;
        }
        assert!(checked > 0);
    }

    #[test]
    fn baked_sound_file_loads() {
        let Ok(sounds) = Sounds::load() else {
            return;
        };
        assert!(sounds.clips.values().flatten().all(|&(_, sound)| sound < sounds.trees.len()));
        // Running plays footsteps; a heavy landing thuds.
        assert!(!sounds.clips["a000_020100"].is_empty());
        assert!(!sounds.clips["a000_202310"].is_empty());
    }
}

#[cfg(test)]
mod mix_tests {
    use super::*;

    #[test]
    fn variation_stays_in_range_and_pitch_is_in_cents() {
        let tree = Tree::Media(Recording { id: 1, db: -10.0, db_range: (-2.0, 0.0), cents: -1200.0, cents_range: (-100.0, 100.0) });
        let mut rng = 0x1234_5678;
        for _ in 0..200 {
            let mut out = Vec::new();
            pick(&tree, &mut rng, &mut out);
            let (_, db, speed) = out[0];
            assert!((-12.0..=-10.0).contains(&db), "{db}");
            // An octave down, give or take a semitone: about half speed.
            assert!(speed > 0.47 && speed < 0.53, "{speed}");
        }
    }
}
