//! A deliberately tiny collision world: flat ground at y = 0 plus solid columns
//! that rise from it. Enough for ledges, stairs and fall damage without a
//! physics engine getting between the input and the character.

use bevy::math::{Vec2, Vec3};

use super::data::STEP_HEIGHT;

#[derive(Clone, Copy, Debug)]
pub struct Block {
    /// Footprint on the XZ plane.
    pub min: Vec2,
    pub max: Vec2,
    pub top: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Level {
    pub blocks: Vec<Block>,
}

impl Level {
    pub fn height(&self, x: f32, z: f32) -> f32 {
        self.blocks
            .iter()
            .filter(|b| x >= b.min.x && x <= b.max.x && z >= b.min.y && z <= b.max.y)
            .map(|b| b.top)
            .fold(0.0, f32::max)
    }

    /// Moves `pos` horizontally by `delta`, sliding along anything too tall to
    /// step onto from the current height.
    pub fn slide(&self, pos: &mut Vec3, delta: Vec3) {
        let reach = pos.y + STEP_HEIGHT;
        let open = |x: f32, z: f32| self.height(x, z) <= reach;
        let (x, z) = (pos.x + delta.x, pos.z + delta.z);
        if open(x, z) {
            pos.x = x;
            pos.z = z;
        } else if open(x, pos.z) {
            pos.x = x;
        } else if open(pos.x, z) {
            pos.z = z;
        }
    }

    pub fn flat() -> Self {
        Self::default()
    }

    /// The test arena: things to jump onto and between, a slope, pillars to
    /// fight around, and one long staircase climbing past every fall-damage
    /// threshold.
    pub fn arena() -> Self {
        let block = |x: (f32, f32), z: (f32, f32), top: f32| Block { min: Vec2::new(x.0, z.0), max: Vec2::new(x.1, z.1), top };
        let mut blocks = vec![
            // Crate: too tall to step onto, low enough to jump onto. Two
            // taller ones behind it make a climb up to the wall.
            block((-8.0, -6.0), (4.0, 6.0), 1.0),
            block((-11.0, -8.0), (4.0, 6.0), 1.75),
            block((-13.0, -11.0), (4.0, 6.0), 2.5),
            // Wall, wide enough to walk along the top of.
            block((-14.0, -13.0), (-4.0, 8.0), 2.5),
            // A platform across a gap from the wall's end.
            block((-15.0, -12.0), (9.5, 13.5), 2.5),
            // Loose crates, one stacked.
            block((9.0, 10.5), (0.0, 1.5), 1.0),
            block((10.5, 12.0), (0.0, 1.5), 2.0),
            // A two-tier dais, each tier low enough to step onto.
            block((-4.0, -1.0), (-13.5, -10.5), 0.3),
            block((-3.5, -1.5), (-13.0, -11.0), 0.6),
            // The platform the slope below leads up to.
            block((12.5, 16.0), (-7.0, -4.0), 1.5),
        ];
        // A row of pillars.
        for x in [-6.0, -2.0, 2.0, 6.0] {
            blocks.push(block((x - 0.5, x + 0.5), (-9.0, -8.0), 3.5));
        }
        // A slope: steps too fine to see as steps.
        for i in 0..15 {
            let x = 8.0 + i as f32 * 0.3;
            blocks.push(block((x, x + 0.3), (-7.0, -4.0), (i + 1) as f32 * 0.1));
        }
        let (rise, run) = (0.25, 0.5);
        let steps = 84;
        for i in 0..steps {
            let x = 8.0 + i as f32 * run;
            blocks.push(Block {
                min: Vec2::new(x, 10.0),
                max: Vec2::new(x + run, 13.0),
                top: (i + 1) as f32 * rise,
            });
        }
        let end = 8.0 + steps as f32 * run;
        blocks.push(Block {
            min: Vec2::new(end, 8.0),
            max: Vec2::new(end + 5.0, 15.0),
            top: steps as f32 * rise,
        });
        Self { blocks }
    }
}
