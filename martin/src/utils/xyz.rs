use std::fmt::{Display, Formatter};

#[derive(Debug, Copy, Clone)]
pub struct TileCoord {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl Display for TileCoord {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "{}/{}/{}", self.z, self.x, self.y)
        } else {
            write!(f, "{},{},{}", self.z, self.x, self.y)
        }
    }
}
