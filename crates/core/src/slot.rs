#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fit {
    Cover,
    Contain,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StripSlotDef {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub rotation_deg: f64,
    pub z_index: i32,
    pub fit: Fit,
    pub prefer_portrait: bool,
    pub prefer_landscape: bool,
    pub polaroid: bool,
    pub horizontal_center_band_frac: Option<f64>,
    pub source_trim_left_frac: Option<f64>,
    pub cover_height_first: bool,
}

impl StripSlotDef {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            x,
            y,
            w,
            h,
            rotation_deg: 0.0,
            z_index: 0,
            fit: Fit::Cover,
            prefer_portrait: false,
            prefer_landscape: false,
            polaroid: false,
            horizontal_center_band_frac: None,
            source_trim_left_frac: None,
            cover_height_first: false,
        }
    }

    pub fn with_rotation(mut self, deg: f64) -> Self {
        self.rotation_deg = deg;
        self
    }

    pub fn with_z(mut self, z: i32) -> Self {
        self.z_index = z;
        self
    }

    pub fn with_prefer_portrait(mut self) -> Self {
        self.prefer_portrait = true;
        self
    }

    pub fn with_prefer_landscape(mut self) -> Self {
        self.prefer_landscape = true;
        self
    }

    pub fn with_polaroid(mut self) -> Self {
        self.polaroid = true;
        self
    }

    pub fn with_horizontal_center_band_frac(mut self, frac: f64) -> Self {
        self.horizontal_center_band_frac = Some(frac);
        self
    }

    pub fn with_source_trim_left_frac(mut self, frac: f64) -> Self {
        self.source_trim_left_frac = Some(frac);
        self
    }

    pub fn with_cover_height_first(mut self) -> Self {
        self.cover_height_first = true;
        self
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }
}
