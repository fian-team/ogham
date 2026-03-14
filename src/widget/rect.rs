#[derive(Debug, Clone)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns `true` if `point` lies within this rectangle (inclusive edges).
    pub fn contains(&self, point: &super::point::Point) -> bool {
        point.x() >= self.x
            && point.x() <= self.x + self.width
            && point.y() >= self.y
            && point.y() <= self.y + self.height
    }
}
