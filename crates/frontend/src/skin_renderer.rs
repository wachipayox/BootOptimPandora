use image::{GenericImageView, ImageFormat, Pixel, RgbaImage};
use schema::minecraft_profile::SkinVariant;

#[derive(Clone, Copy, Debug)]
struct V3 {
    x: f64,
    y: f64,
    z: f64,
}

impl V3 {
    const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    fn dot(&self, other: V3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }
}

/// 3×3 rotation matrix stored as rows.
struct Mat3([[f64; 3]; 3]);

impl Mat3 {
    fn rotation_yx(ry: f64, rx: f64) -> Self {
        let (sy, cy) = ry.sin_cos();
        let (sx, cx) = rx.sin_cos();
        Self([[cy, 0.0, sy], [sx * sy, cx, -sx * cy], [-cx * sy, sx, cx * cy]])
    }

    fn rotation_x(rx: f64) -> Self {
        let (sx, cx) = rx.sin_cos();
        Self([[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]])
    }

    fn transform(&self, v: V3) -> V3 {
        let r = &self.0;
        V3 {
            x: r[0][0] * v.x + r[0][1] * v.y + r[0][2] * v.z,
            y: r[1][0] * v.x + r[1][1] * v.y + r[1][2] * v.z,
            z: r[2][0] * v.x + r[2][1] * v.y + r[2][2] * v.z,
        }
    }

    fn transform_with_offset(&self, v: V3, o: V3) -> V3 {
        let r = &self.0;
        let x = v.x - o.x;
        let y = v.y - o.y;
        let z = v.z - o.z;
        V3 {
            x: r[0][0] * x + r[0][1] * y + r[0][2] * z + o.x,
            y: r[1][0] * x + r[1][1] * y + r[1][2] * z + o.y,
            z: r[2][0] * x + r[2][1] * y + r[2][2] * z + o.z,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BodyPartType {
    Head,
    Body,
    RightArm,
    LeftArm,
    RightLeg,
    LeftLeg,
    HeadOverlay,
    BodyOverlay,
    RightArmOverlay,
    LeftArmOverlay,
    RightLegOverlay,
    LeftLegOverlay,
    Cape,
}

impl BodyPartType {
    pub const fn inflate(self) -> f64 {
        match self {
            Self::HeadOverlay
            | Self::RightArmOverlay
            | Self::LeftArmOverlay
            | Self::RightLegOverlay
            | Self::LeftLegOverlay => 0.25,
            Self::BodyOverlay => 0.24,
            _ => 0.0,
        }
    }

    pub const fn allow_transparency(self) -> bool {
        match self {
            Self::HeadOverlay
            | Self::RightArmOverlay
            | Self::LeftArmOverlay
            | Self::RightLegOverlay
            | Self::LeftLegOverlay
            | Self::BodyOverlay => true,
            _ => false,
        }
    }

    pub const fn sway_time_mult(self) -> f64 {
        if let Self::Cape = self { 1.0 } else { 4.0 }
    }

    pub const fn sway_strength(self) -> f64 {
        match self {
            Self::RightArm | Self::RightArmOverlay | Self::LeftLeg | Self::LeftLegOverlay => 1.0,
            Self::LeftArm | Self::LeftArmOverlay | Self::RightLeg | Self::RightLegOverlay => -1.0,
            Self::Cape => 0.25,
            _ => 0.0,
        }
    }

    pub const fn pitch_offset(self) -> f64 {
        if let Self::Cape = self {
            18.75_f64.to_radians()
        } else {
            0.0
        }
    }
}

struct BodyPartDef {
    min: V3,
    max: V3,
    pivot: Option<V3>,
    tx: f64,
    ty: f64,
    flip_x: bool,
    part_type: BodyPartType,
}

impl BodyPartDef {
    pub const fn to_quads(&self) -> [Quad; 6] {
        let inflate = self.part_type.inflate();
        let allow_transparency = self.part_type.allow_transparency();

        let w = self.max.x - self.min.x;
        let h = self.max.y - self.min.y;
        let d = self.max.z - self.min.z;
        let x0 = self.min.x - inflate;
        let y0 = self.min.y - inflate;
        let z0 = self.min.z - inflate;
        let x1 = self.max.x + inflate;
        let y1 = self.max.y + inflate;
        let z1 = self.max.z + inflate;
        let tx = self.tx;
        let ty = self.ty;
        let flip_x = self.flip_x;
        let flip_z = matches!(self.part_type, BodyPartType::Cape);

        let mut quads = [
            // Front face (+Z) – texture region at (tx+d, ty+d) size w×h
            Quad {
                verts: [
                    V3::new(x0, y1, z1),
                    V3::new(x1, y1, z1),
                    V3::new(x1, y0, z1),
                    V3::new(x0, y0, z1),
                ],
                uvs: [
                    (tx + d, ty + d),
                    (tx + d + w, ty + d),
                    (tx + d + w, ty + d + h),
                    (tx + d, ty + d + h),
                ],
                normal: V3::new(0.0, 0.0, 1.0),
                allow_transparency,
            }
            .flip_uv_horz(flip_x),
            // Back face (-Z) – texture at (tx+2d+w, ty+d) size w×h
            Quad {
                verts: [
                    V3::new(x1, y1, z0),
                    V3::new(x0, y1, z0),
                    V3::new(x0, y0, z0),
                    V3::new(x1, y0, z0),
                ],
                uvs: [
                    (tx + 2.0 * d + w, ty + d),
                    (tx + 2.0 * d + 2.0 * w, ty + d),
                    (tx + 2.0 * d + 2.0 * w, ty + d + h),
                    (tx + 2.0 * d + w, ty + d + h),
                ],
                normal: V3::new(0.0, 0.0, -1.0),
                allow_transparency,
            }
            .flip_uv_horz(flip_x),
            // Right face (-X, player's right) – texture at (tx, ty+d) size d×h
            Quad {
                verts: [
                    V3::new(x0, y1, z1),
                    V3::new(x0, y1, z0),
                    V3::new(x0, y0, z0),
                    V3::new(x0, y0, z1),
                ],
                uvs: [(tx + d, ty + d), (tx, ty + d), (tx, ty + d + h), (tx + d, ty + d + h)],
                normal: V3::new(-1.0, 0.0, 0.0),
                allow_transparency,
            }
            .flip_uv_horz(flip_z),
            // Left face (+X, player's left) – texture at (tx+d+w, ty+d) size d×h
            Quad {
                verts: [
                    V3::new(x1, y1, z0),
                    V3::new(x1, y1, z1),
                    V3::new(x1, y0, z1),
                    V3::new(x1, y0, z0),
                ],
                uvs: [
                    (tx + 2.0 * d + w, ty + d),
                    (tx + d + w, ty + d),
                    (tx + d + w, ty + d + h),
                    (tx + 2.0 * d + w, ty + d + h),
                ],
                normal: V3::new(1.0, 0.0, 0.0),
                allow_transparency,
            }
            .flip_uv_horz(flip_z),
            // Top face (+Y) – texture at (tx+d, ty) size w×d
            Quad {
                verts: [
                    V3::new(x0, y1, z0),
                    V3::new(x1, y1, z0),
                    V3::new(x1, y1, z1),
                    V3::new(x0, y1, z1),
                ],
                uvs: [(tx + d, ty), (tx + d + w, ty), (tx + d + w, ty + d), (tx + d, ty + d)],
                normal: V3::new(0.0, 1.0, 0.0),
                allow_transparency,
            }
            .flip_uv_horz(flip_x)
            .flip_uv_vert(flip_z),
            // Bottom face (-Y) – texture at (tx+d+w, ty) size w×d
            Quad {
                verts: [
                    V3::new(x1, y0, z0),
                    V3::new(x0, y0, z0),
                    V3::new(x0, y0, z1),
                    V3::new(x1, y0, z1),
                ],
                uvs: [
                    (tx + d + 2.0 * w, ty),
                    (tx + d + w, ty),
                    (tx + d + w, ty + d),
                    (tx + d + 2.0 * w, ty + d),
                ],
                normal: V3::new(0.0, -1.0, 0.0),
                allow_transparency,
            }
            .flip_uv_horz(flip_x)
            .flip_uv_vert(flip_z),
        ];

        if flip_x {
            unsafe {
                std::ptr::swap(&raw mut quads[2].uvs, &raw mut quads[3].uvs);
            }
        }
        if flip_z {
            unsafe {
                std::ptr::swap(&raw mut quads[0].uvs, &raw mut quads[1].uvs);
            }
        }

        quads
    }

    fn add_projected_quads(
        &self,
        projected_quads: &mut Vec<ProjectedQuad>,
        rot: &Mat3,
        light0: V3,
        light1: V3,
        sway_progress: f64,
    ) {
        let mut quads = self.to_quads();
        let sway_strength = self.part_type.sway_strength();
        let sway_time_mult = self.part_type.sway_time_mult();
        let pitch_offset = self.part_type.pitch_offset();

        let pitch =
            -15.0_f64.to_radians() * sway_strength * (sway_progress * std::f64::consts::TAU * sway_time_mult).sin()
                + pitch_offset;

        if pitch != 0.0 {
            let transform = Mat3::rotation_x(pitch);
            for quad in &mut quads {
                let pivot = self.pivot.unwrap_or_else(|| {
                    V3::new(
                        self.min.x / 2.0 + self.max.x / 2.0,
                        self.min.y / 2.0 + self.max.y / 2.0,
                        self.min.z / 2.0 + self.max.z / 2.0,
                    )
                });
                for vert in &mut quad.verts {
                    *vert = transform.transform_with_offset(*vert, pivot);
                }
                quad.normal = transform.transform(quad.normal);
            }
        }

        for quad in &quads {
            let mut rn = rot.transform(quad.normal);

            if rn.z <= 0.0 {
                if !quad.allow_transparency {
                    // Cull back facing quads
                    continue;
                } else {
                    // Flip for correct lighting
                    rn.x *= -1.0;
                    rn.y *= -1.0;
                    rn.z *= -1.0;
                }
            }

            let dot0 = rn.dot(light0).clamp(0.0, 1.0);
            let dot1 = rn.dot(light1).clamp(0.0, 1.0);
            let accum = ((dot0 + dot1).min(1.0) * 0.4 + 0.6).clamp(0.0, 1.0);
            let shade = (accum * 255.0) as u8;

            // Transform vertices
            let mut screen_verts = [V3::new(0.0, 0.0, 0.0); 4];
            let mut avg_z = 0.0;
            for (j, v) in quad.verts.iter().enumerate() {
                let mut rv = rot.transform(*v);
                rv.y *= -1.0; // flip for screenspace (y down)
                screen_verts[j] = rv;
                avg_z += rv.z;
            }
            avg_z /= 4.0;

            projected_quads.push(ProjectedQuad {
                verts: screen_verts,
                uvs: quad.uvs,
                avg_z,
                allow_transparency: quad.allow_transparency,
                shade,
                part_type: self.part_type,
            });
        }
    }
}

static PLAYER_MODEL: &'static [BodyPartDef] = &[
    // Head
    BodyPartDef {
        min: V3::new(-4.0, 8.0, -4.0),
        max: V3::new(4.0, 16.0, 4.0),
        pivot: None,
        tx: 0.0,
        ty: 0.0,
        flip_x: false,
        part_type: BodyPartType::Head,
    },
    // Body
    BodyPartDef {
        min: V3::new(-4.0, -4.0, -2.0),
        max: V3::new(4.0, 8.0, 2.0),
        pivot: None,
        tx: 16.0,
        ty: 16.0,
        flip_x: false,
        part_type: BodyPartType::Body,
    },
    // Right Arm
    BodyPartDef {
        min: V3::new(-8.0, -4.0, -2.0),
        max: V3::new(-4.0, 8.0, 2.0),
        pivot: Some(V3::new(-4.0, 8.0, 0.0)),
        tx: 40.0,
        ty: 16.0,
        flip_x: false,
        part_type: BodyPartType::RightArm,
    },
    // Left Arm
    BodyPartDef {
        min: V3::new(4.0, -4.0, -2.0),
        max: V3::new(8.0, 8.0, 2.0),
        pivot: Some(V3::new(4.0, 8.0, 0.0)),
        tx: 32.0,
        ty: 48.0,
        flip_x: false,
        part_type: BodyPartType::LeftArm,
    },
    // Right Leg
    BodyPartDef {
        min: V3::new(-4.0, -16.0, -2.0),
        max: V3::new(0.0, -4.0, 2.0),
        pivot: Some(V3::new(-2.0, -4.0, 0.0)),
        tx: 0.0,
        ty: 16.0,
        flip_x: false,
        part_type: BodyPartType::RightLeg,
    },
    // Left Leg
    BodyPartDef {
        min: V3::new(0.0, -16.0, -2.0),
        max: V3::new(4.0, -4.0, 2.0),
        pivot: Some(V3::new(2.0, -4.0, 0.0)),
        tx: 16.0,
        ty: 48.0,
        flip_x: false,
        part_type: BodyPartType::LeftLeg,
    },
    // Head overlay (hat)
    BodyPartDef {
        min: V3::new(-4.0, 8.0, -4.0),
        max: V3::new(4.0, 16.0, 4.0),
        pivot: None,
        tx: 32.0,
        ty: 0.0,
        flip_x: false,
        part_type: BodyPartType::HeadOverlay,
    },
    // Body overlay
    BodyPartDef {
        min: V3::new(-4.0, -4.0, -2.0),
        max: V3::new(4.0, 8.0, 2.0),
        pivot: None,
        tx: 16.0,
        ty: 32.0,
        flip_x: false,
        part_type: BodyPartType::BodyOverlay,
    },
    // Right Arm overlay
    BodyPartDef {
        min: V3::new(-8.0, -4.0, -2.0),
        max: V3::new(-4.0, 8.0, 2.0),
        pivot: Some(V3::new(-4.0, 8.0, 0.0)),
        tx: 40.0,
        ty: 32.0,
        flip_x: false,
        part_type: BodyPartType::RightArmOverlay,
    },
    // Left Arm overlay
    BodyPartDef {
        min: V3::new(4.0, -4.0, -2.0),
        max: V3::new(8.0, 8.0, 2.0),
        pivot: Some(V3::new(4.0, 8.0, 0.0)),
        tx: 48.0,
        ty: 48.0,
        flip_x: false,
        part_type: BodyPartType::LeftArmOverlay,
    },
    // Right Leg overlay
    BodyPartDef {
        min: V3::new(-4.0, -16.0, -2.0),
        max: V3::new(0.0, -4.0, 2.0),
        pivot: Some(V3::new(-2.0, -4.0, 0.0)),
        tx: 0.0,
        ty: 32.0,
        flip_x: false,
        part_type: BodyPartType::RightLegOverlay,
    },
    // Left Leg overlay
    BodyPartDef {
        min: V3::new(0.0, -16.0, -2.0),
        max: V3::new(4.0, -4.0, 2.0),
        pivot: Some(V3::new(2.0, -4.0, 0.0)),
        tx: 0.0,
        ty: 48.0,
        flip_x: false,
        part_type: BodyPartType::LeftLegOverlay,
    },
];

static SLIM_PLAYER_MODEL: &'static [BodyPartDef] = &[
    // Head
    BodyPartDef {
        min: V3::new(-4.0, 8.0, -4.0),
        max: V3::new(4.0, 16.0, 4.0),
        pivot: None,
        tx: 0.0,
        ty: 0.0,
        flip_x: false,
        part_type: BodyPartType::Head,
    },
    // Body
    BodyPartDef {
        min: V3::new(-4.0, -4.0, -2.0),
        max: V3::new(4.0, 8.0, 2.0),
        pivot: None,
        tx: 16.0,
        ty: 16.0,
        flip_x: false,
        part_type: BodyPartType::Body,
    },
    // Right Arm
    BodyPartDef {
        min: V3::new(-7.0, -4.0, -2.0),
        max: V3::new(-4.0, 8.0, 2.0),
        pivot: Some(V3::new(-4.0, 8.0, 0.0)),
        tx: 40.0,
        ty: 16.0,
        flip_x: false,
        part_type: BodyPartType::RightArm,
    },
    // Left Arm
    BodyPartDef {
        min: V3::new(4.0, -4.0, -2.0),
        max: V3::new(7.0, 8.0, 2.0),
        pivot: Some(V3::new(4.0, 8.0, 0.0)),
        tx: 32.0,
        ty: 48.0,
        flip_x: false,
        part_type: BodyPartType::LeftArm,
    },
    // Right Leg
    BodyPartDef {
        min: V3::new(-4.0, -16.0, -2.0),
        max: V3::new(0.0, -4.0, 2.0),
        pivot: Some(V3::new(-2.0, -4.0, 0.0)),
        tx: 0.0,
        ty: 16.0,
        flip_x: false,
        part_type: BodyPartType::RightLeg,
    },
    // Left Leg
    BodyPartDef {
        min: V3::new(0.0, -16.0, -2.0),
        max: V3::new(4.0, -4.0, 2.0),
        pivot: Some(V3::new(2.0, -4.0, 0.0)),
        tx: 16.0,
        ty: 48.0,
        flip_x: false,
        part_type: BodyPartType::LeftLeg,
    },
    // Head overlay (hat)
    BodyPartDef {
        min: V3::new(-4.0, 8.0, -4.0),
        max: V3::new(4.0, 16.0, 4.0),
        pivot: None,
        tx: 32.0,
        ty: 0.0,
        flip_x: false,
        part_type: BodyPartType::HeadOverlay,
    },
    // Body overlay
    BodyPartDef {
        min: V3::new(-4.0, -4.0, -2.0),
        max: V3::new(4.0, 8.0, 2.0),
        pivot: None,
        tx: 16.0,
        ty: 32.0,
        flip_x: false,
        part_type: BodyPartType::BodyOverlay,
    },
    // Right Arm overlay
    BodyPartDef {
        min: V3::new(-7.0, -4.0, -2.0),
        max: V3::new(-4.0, 8.0, 2.0),
        pivot: Some(V3::new(-4.0, 8.0, 0.0)),
        tx: 40.0,
        ty: 32.0,
        flip_x: false,
        part_type: BodyPartType::RightArmOverlay,
    },
    // Left Arm overlay
    BodyPartDef {
        min: V3::new(4.0, -4.0, -2.0),
        max: V3::new(7.0, 8.0, 2.0),
        pivot: Some(V3::new(4.0, 8.0, 0.0)),
        tx: 48.0,
        ty: 48.0,
        flip_x: false,
        part_type: BodyPartType::LeftArmOverlay,
    },
    // Right Leg overlay
    BodyPartDef {
        min: V3::new(-4.0, -16.0, -2.0),
        max: V3::new(0.0, -4.0, 2.0),
        pivot: Some(V3::new(-2.0, -4.0, 0.0)),
        tx: 0.0,
        ty: 32.0,
        flip_x: false,
        part_type: BodyPartType::RightLegOverlay,
    },
    // Left Leg overlay
    BodyPartDef {
        min: V3::new(0.0, -16.0, -2.0),
        max: V3::new(4.0, -4.0, 2.0),
        pivot: Some(V3::new(2.0, -4.0, 0.0)),
        tx: 0.0,
        ty: 48.0,
        flip_x: false,
        part_type: BodyPartType::LeftLegOverlay,
    },
];

static LEGACY_PLAYER_MODEL: &'static [BodyPartDef] = &[
    // Head
    BodyPartDef {
        min: V3::new(-4.0, 8.0, -4.0),
        max: V3::new(4.0, 16.0, 4.0),
        pivot: None,
        tx: 0.0,
        ty: 0.0,
        flip_x: false,
        part_type: BodyPartType::Head,
    },
    // Body
    BodyPartDef {
        min: V3::new(-4.0, -4.0, -2.0),
        max: V3::new(4.0, 8.0, 2.0),
        pivot: None,
        tx: 16.0,
        ty: 16.0,
        flip_x: false,
        part_type: BodyPartType::Body,
    },
    // Right Arm
    BodyPartDef {
        min: V3::new(-8.0, -4.0, -2.0),
        max: V3::new(-4.0, 8.0, 2.0),
        pivot: Some(V3::new(-4.0, 8.0, 0.0)),
        tx: 40.0,
        ty: 16.0,
        flip_x: false,
        part_type: BodyPartType::RightArm,
    },
    // Left Arm
    BodyPartDef {
        min: V3::new(4.0, -4.0, -2.0),
        max: V3::new(8.0, 8.0, 2.0),
        pivot: Some(V3::new(4.0, 8.0, 0.0)),
        tx: 40.0,
        ty: 16.0,
        flip_x: true,
        part_type: BodyPartType::LeftArm,
    },
    // Right Leg
    BodyPartDef {
        min: V3::new(-4.0, -16.0, -2.0),
        max: V3::new(0.0, -4.0, 2.0),
        pivot: Some(V3::new(-2.0, -4.0, 0.0)),
        tx: 0.0,
        ty: 16.0,
        flip_x: false,
        part_type: BodyPartType::RightLeg,
    },
    // Left Leg
    BodyPartDef {
        min: V3::new(0.0, -16.0, -2.0),
        max: V3::new(4.0, -4.0, 2.0),
        pivot: Some(V3::new(2.0, -4.0, 0.0)),
        tx: 0.0,
        ty: 16.0,
        flip_x: true,
        part_type: BodyPartType::LeftLeg,
    },
    // Head overlay (hat)
    BodyPartDef {
        min: V3::new(-4.0, 8.0, -4.0),
        max: V3::new(4.0, 16.0, 4.0),
        pivot: None,
        tx: 32.0,
        ty: 0.0,
        flip_x: false,
        part_type: BodyPartType::HeadOverlay,
    },
];

struct Quad {
    verts: [V3; 4],
    uvs: [(f64, f64); 4],
    normal: V3,
    allow_transparency: bool,
}

impl Quad {
    pub const fn flip_uv_horz(mut self, flip: bool) -> Self {
        if flip {
            self.uvs.swap(0, 1);
            self.uvs.swap(2, 3);
        }
        self
    }

    pub const fn flip_uv_vert(mut self, flip: bool) -> Self {
        if flip {
            self.uvs.swap(0, 3);
            self.uvs.swap(1, 2);
        }
        self
    }
}

struct ProjectedQuad {
    verts: [V3; 4],
    uvs: [(f64, f64); 4],
    avg_z: f64,
    allow_transparency: bool,
    shade: u8,
    part_type: BodyPartType,
}

/// 2D edge function: positive when `p` is to the left of edge `a→b`.
#[inline]
fn edge(ax: f64, ay: f64, bx: f64, by: f64, px: f64, py: f64) -> f64 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}

// Exact integer shading, shared by all frames/previews. Only the rows for the
// current face lighting are touched; no per-frame allocation or approximation.
static SHADED_CHANNEL: [[u8; 256]; 256] = {
    let mut table = [[0; 256]; 256];
    let mut shade = 0;
    while shade < 256 {
        let mut channel = 0;
        while channel < 256 {
            table[shade][channel] = ((channel as u16 * shade as u16) / 255) as u8;
            channel += 1;
        }
        shade += 1;
    }
    table
};

/// Broad-phase rejection only: intersect the triangle with a two-pixel-high band,
/// then expand horizontally. The original pixel-centre barycentric test below
/// still decides coverage; no edge tolerance, interpolation or shading changes.
/// Nonfinite/ambiguous geometry fails open to the original bounding rectangle.
fn conservative_row_span(verts: [V3; 3], cy: f64, min_x: i32, max_x: i32) -> (i32, i32) {
    let bottom = cy - 1.0;
    let top = cy + 1.0;
    let mut left = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    for i in 0..3 {
        let a = verts[i];
        let b = verts[(i + 1) % 3];
        if !a.x.is_finite() || !a.y.is_finite() {
            return (min_x, max_x);
        }
        if a.y >= bottom && a.y <= top {
            left = left.min(a.x);
            right = right.max(a.x);
        }
        if a.y != b.y {
            for y in [bottom, top] {
                if y >= a.y.min(b.y) && y <= a.y.max(b.y) {
                    let x = a.x + ((y - a.y) / (b.y - a.y)) * (b.x - a.x);
                    left = left.min(x);
                    right = right.max(x);
                }
            }
        }
    }
    if !left.is_finite() || !right.is_finite() {
        return (min_x, max_x);
    }
    (((left - 1.0).floor() as i32).max(min_x), ((right + 1.0).ceil() as i32).min(max_x))
}

/// Rasterize a single triangle with texture mapping and z-buffering.
fn rasterize_triangle(
    // Screen-space vertices (x, y, z for depth)
    v0: V3,
    v1: V3,
    v2: V3,
    // Texture coordinates (absolute pixel coords in skin)
    uv0: (f64, f64),
    uv1: (f64, f64),
    uv2: (f64, f64),
    skin: &RgbaImage,
    output: &mut RgbaImage,
    zbuf: &mut [f64],
    allow_transparency: bool,
    shade: u8,
) {
    let skin_w = skin.width();
    let skin_h = skin.height();
    let out_w = output.width();
    let out_h = output.height();

    // Bounding box (clamped to output)
    let min_x = v0.x.min(v1.x).min(v2.x).floor().max(0.0) as i32;
    let max_x = v0.x.max(v1.x).max(v2.x).ceil().min(out_w as f64 - 1.0) as i32;
    let min_y = v0.y.min(v1.y).min(v2.y).floor().max(0.0) as i32;
    let max_y = v0.y.max(v1.y).max(v2.y).ceil().min(out_h as f64 - 1.0) as i32;

    let area = edge(v0.x, v0.y, v1.x, v1.y, v2.x, v2.y);
    if area.abs() < 0.001 {
        return; // degenerate
    }
    let inv_area = 1.0 / area;

    for py in min_y..=max_y {
        let (row_min, row_max) = conservative_row_span([v0, v1, v2], py as f64 + 0.5, min_x, max_x);
        for px in row_min..=row_max {
            let cx = px as f64 + 0.5;
            let cy = py as f64 + 0.5;

            let w0 = edge(v1.x, v1.y, v2.x, v2.y, cx, cy) * inv_area;
            let w1 = edge(v2.x, v2.y, v0.x, v0.y, cx, cy) * inv_area;
            let w2 = 1.0 - w0 - w1;

            if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                let z = w0 * v0.z + w1 * v1.z + w2 * v2.z;
                let idx = (py as u32 * out_w + px as u32) as usize;

                if z > zbuf[idx] {
                    let u = w0 * uv0.0 + w1 * uv1.0 + w2 * uv2.0;
                    let v = w0 * uv0.1 + w1 * uv1.1 + w2 * uv2.1;

                    let tx = (u.floor() as u32).min(skin_w - 1);
                    let ty = (v.floor() as u32).min(skin_h - 1);
                    let mut pixel = *skin.get_pixel(tx, ty);

                    if !allow_transparency {
                        pixel[3] = 0xFF;
                    }
                    if pixel[3] == 0 {
                        continue;
                    }

                    pixel[0] = SHADED_CHANNEL[shade as usize][pixel[0] as usize];
                    pixel[1] = SHADED_CHANNEL[shade as usize][pixel[1] as usize];
                    pixel[2] = SHADED_CHANNEL[shade as usize][pixel[2] as usize];

                    if pixel[3] == 0xFF {
                        output.put_pixel(px as u32, py as u32, pixel);
                    } else {
                        output.get_pixel_mut(px as u32, py as u32).blend(&pixel);
                    }
                    zbuf[idx] = z;
                }
            }
        }
    }
}

impl ProjectedQuad {
    fn rasterize(&self, skin: &RgbaImage, output: &mut RgbaImage, zbuf: &mut [f64]) {
        // Triangle 1: v0, v1, v2
        rasterize_triangle(
            self.verts[0],
            self.verts[1],
            self.verts[2],
            self.uvs[0],
            self.uvs[1],
            self.uvs[2],
            skin,
            output,
            zbuf,
            self.allow_transparency,
            self.shade,
        );
        // Triangle 2: v0, v2, v3
        rasterize_triangle(
            self.verts[0],
            self.verts[2],
            self.verts[3],
            self.uvs[0],
            self.uvs[2],
            self.uvs[3],
            skin,
            output,
            zbuf,
            self.allow_transparency,
            self.shade,
        );
    }
}

fn collect_quads(
    is_legacy: bool,
    is_slim: bool,
    add_cape: bool,
    yaw_deg: f64,
    pitch_deg: f64,
    sway_progress: f64,
) -> Vec<ProjectedQuad> {
    let rot = Mat3::rotation_yx(yaw_deg.to_radians(), pitch_deg.to_radians());

    let parts = if is_legacy {
        LEGACY_PLAYER_MODEL
    } else if is_slim {
        SLIM_PLAYER_MODEL
    } else {
        PLAYER_MODEL
    };
    let mut projected_quads: Vec<ProjectedQuad> = Vec::new();

    let light0 = V3::new(0.16169041669088866, 0.8084520834544432, -0.5659164584181102);
    let light1 = V3::new(-0.16169041669088866, 0.8084520834544432, 0.5659164584181102);

    for part in parts.iter() {
        part.add_projected_quads(&mut projected_quads, &rot, light0, light1, sway_progress);
    }

    if add_cape {
        BodyPartDef {
            min: V3::new(-5.0, -8.0, -3.0),
            max: V3::new(5.0, 8.0, -2.0),
            pivot: Some(V3::new(0.0, 8.0, -2.0)),
            tx: 0.0,
            ty: 0.0,
            flip_x: false,
            part_type: BodyPartType::Cape,
        }
        .add_projected_quads(&mut projected_quads, &rot, light0, light1, sway_progress);
    }

    projected_quads
}

pub fn determine_skin_variant(skin_png: &[u8]) -> Option<SkinVariant> {
    let skin = image::load_from_memory_with_format(skin_png, ImageFormat::Png).ok()?;
    if skin.width() != 64 || !matches!(skin.height(), 32 | 64) {
        return None;
    }
    let is_legacy = skin.height() == 32;
    if !is_legacy && skin.get_pixel(54, 20)[3] < 20 {
        Some(SkinVariant::Slim)
    } else {
        Some(SkinVariant::Classic)
    }
}

pub fn render_skin_3d(
    skin_png_bytes: &[u8],
    cape_png_bytes: Option<&[u8]>,
    variant: SkinVariant,
    out_width: u32,
    out_height: u32,
    yaw_deg: f64,
    pitch_deg: f64,
    sway_progress: f64,
    y_offset: f64,
    zoom: f64,
) -> Option<RgbaImage> {
    let textures = SkinTextures::decode(skin_png_bytes, cape_png_bytes)?;
    render_skin_textures(&textures, variant, out_width, out_height, yaw_deg, pitch_deg, sway_progress, y_offset, zoom)
}

/// Decoded textures belong to one preview; retaining them avoids PNG decoding on every frame.
pub struct SkinTextures {
    skin: RgbaImage,
    cape: Option<RgbaImage>,
    skin_alpha: AlphaCoverage,
}

/// Summed-area map of nonzero alpha; one small map per decoded 64-pixel skin.
/// Conservative UV rectangles include their borders, so a skipped face cannot sample
/// any visible texel, including texels shared with a neighbouring texture region.
struct AlphaCoverage {
    stride: usize,
    counts: Vec<u16>,
}

impl AlphaCoverage {
    fn new(skin: &RgbaImage) -> Self {
        let stride = skin.width() as usize + 1;
        let mut counts = vec![0u16; stride * (skin.height() as usize + 1)];
        for y in 0..skin.height() as usize {
            let mut row = 0;
            for x in 0..skin.width() as usize {
                row += u16::from(skin.get_pixel(x as u32, y as u32)[3] != 0);
                counts[(y + 1) * stride + x + 1] = counts[y * stride + x + 1] + row;
            }
        }
        Self { stride, counts }
    }

    fn contains_visible_texel(&self, quad: &ProjectedQuad, skin: &RgbaImage) -> bool {
        let mut min_u = f64::INFINITY;
        let mut min_v = f64::INFINITY;
        let mut max_u = f64::NEG_INFINITY;
        let mut max_v = f64::NEG_INFINITY;
        for &(u, v) in &quad.uvs {
            min_u = min_u.min(u);
            min_v = min_v.min(v);
            max_u = max_u.max(u);
            max_v = max_v.max(v);
        }
        // Include a texel outside either edge as a guard for interpolation rounding.
        let x0 = (min_u.floor() - 1.0).max(0.0).min((skin.width() - 1) as f64) as usize;
        let y0 = (min_v.floor() - 1.0).max(0.0).min((skin.height() - 1) as f64) as usize;
        let x1 = (max_u.ceil() + 1.0).max(0.0).min((skin.width() - 1) as f64) as usize + 1;
        let y1 = (max_v.ceil() + 1.0).max(0.0).min((skin.height() - 1) as f64) as usize + 1;
        let count = self.counts[y1 * self.stride + x1] as i32
            - self.counts[y0 * self.stride + x1] as i32
            - self.counts[y1 * self.stride + x0] as i32
            + self.counts[y0 * self.stride + x0] as i32;
        count != 0
    }
}

impl SkinTextures {
    pub fn decode(skin_png: &[u8], cape_png: Option<&[u8]>) -> Option<Self> {
        let skin = image::load_from_memory_with_format(skin_png, ImageFormat::Png).ok()?;
        if skin.width() != 64 || !matches!(skin.height(), 32 | 64) {
            return None;
        }
        let cape = cape_png
            .and_then(|bytes| image::load_from_memory_with_format(bytes, ImageFormat::Png).ok())
            .map(|image| image.to_rgba8());
        let skin = skin.to_rgba8();
        let skin_alpha = AlphaCoverage::new(&skin);
        Some(Self { skin, cape, skin_alpha })
    }
}

pub fn render_skin_textures(
    textures: &SkinTextures,
    variant: SkinVariant,
    out_width: u32,
    out_height: u32,
    yaw_deg: f64,
    pitch_deg: f64,
    sway_progress: f64,
    y_offset: f64,
    zoom: f64,
) -> Option<RgbaImage> {
    render_skin_textures_with_scratch(textures, variant, out_width, out_height, yaw_deg, pitch_deg,
        sway_progress, y_offset, zoom, &mut SkinRenderScratch::default())
}

/// Owned by one preview and passed to its single renderer worker. Depth is always
/// cleared before each frame; retaining storage avoids allocating a native-size
/// depth buffer thirty times per second. It is released with the preview entity.
#[derive(Default)]
pub struct SkinRenderScratch {
    depth: Vec<f64>,
}

pub fn render_skin_textures_with_scratch(
    textures: &SkinTextures,
    variant: SkinVariant,
    out_width: u32,
    out_height: u32,
    yaw_deg: f64,
    pitch_deg: f64,
    sway_progress: f64,
    y_offset: f64,
    zoom: f64,
    scratch: &mut SkinRenderScratch,
) -> Option<RgbaImage> {
    if out_width == 0 || out_height == 0 || out_width > 2048 || out_height > 2048 {
        return None;
    }
    let skin = &textures.skin;
    let cape = textures.cape.as_ref();

    let is_legacy = skin.height() == 32;
    if skin.width() != 64 {
        return None;
    }
    if skin.height() != 64 && !is_legacy {
        return None;
    }
    let is_slim = match variant {
        SkinVariant::Classic => false,
        SkinVariant::Slim => true,
        SkinVariant::Other => !is_legacy && skin.get_pixel(54, 20)[3] < 20,
    };

    let mut projected_quads = collect_quads(is_legacy, is_slim, cape.is_some(), yaw_deg, pitch_deg, sway_progress);

    // Sort back-to-front (painter's algorithm): smaller Z = further from camera = draw first
    projected_quads.sort_by(|a, b| a.avg_z.partial_cmp(&b.avg_z).unwrap_or(std::cmp::Ordering::Equal));

    let scale = (out_width as f64 / MAX_WIDTH_AT_ANY_ANGLE).min(out_height as f64 / MAX_HEIGHT_AT_ANY_ANGLE) * zoom;
    let offset_x = out_width as f64 / 2.0;
    let offset_y = out_height as f64 / 2.0 + y_offset * scale;

    // Create output image (transparent background)
    let mut output = RgbaImage::new(out_width, out_height);
    scratch.depth.resize((out_width * out_height) as usize, f64::MIN);
    scratch.depth.fill(f64::MIN);
    let zbuf = &mut scratch.depth;

    // Rasterize each quad
    for mut projected_quad in projected_quads {
        if projected_quad.allow_transparency
            && projected_quad.part_type != BodyPartType::Cape
            && !textures.skin_alpha.contains_visible_texel(&projected_quad, skin) {
            continue;
        }
        let verts = &mut projected_quad.verts;
        for i in 0..4 {
            verts[i] = V3::new(verts[i].x * scale + offset_x, verts[i].y * scale + offset_y, verts[i].z);
        }
        if projected_quad.part_type == BodyPartType::Cape {
            if let Some(cape) = &cape {
                projected_quad.rasterize(cape, &mut output, zbuf);
            }
        } else {
            projected_quad.rasterize(skin, &mut output, zbuf);
        }
    }

    Some(output)
}

// Constants calculated by brute force
const MAX_CAPE_ANGLE_SWAY_PROGRESS: f64 = 3.0 / 4.0;
const MAX_WIDTH_AT_ANY_ANGLE: f64 = 20.407198535851574; // yaw=60.65789523301863, pitch=0
const MAX_HEIGHT_AT_ANY_ANGLE: f64 = 34.65183977799737; // yaw=45, pitch=20.29798422703834
pub const ASPECT_RATIO: f64 = MAX_WIDTH_AT_ANY_ANGLE / MAX_HEIGHT_AT_ANY_ANGLE;

#[cfg(test)]
mod preview_tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn integer_shading_lookup_is_exact() {
        for shade in 0..256 {
            for channel in 0..256 {
                assert_eq!(SHADED_CHANNEL[shade][channel], ((channel as u16 * shade as u16) / 255) as u8);
            }
        }
    }

    #[test]
    fn row_span_keeps_every_pixel_accepted_by_original_edge_test() {
        let triangles = [
            [(0.0, 0.0), (90.0, 0.0), (90.0, 150.0)],
            [(10.5, 10.5), (10.50001, 140.5), (90.5, 140.5)],
            [(49.4999, 0.0), (49.5001, 159.0), (49.4999, 159.0)],
            [(-200.0, -300.0), (350.0, 0.0), (80.0, 320.0)],
            [(-9000.0, -9000.0), (9000.0, 10.5), (10.5, 9000.0)],
        ];
        for triangle in triangles {
            for reverse in [false, true] {
                let mut verts = triangle.map(|(x, y)| V3::new(x, y, 0.0));
                if reverse { verts.swap(0, 2); }
                let [v0, v1, v2] = verts;
                let inv_area = 1.0 / edge(v0.x, v0.y, v1.x, v1.y, v2.x, v2.y);
                for py in 0..160 {
                    let cy = py as f64 + 0.5;
                    let (left, right) = conservative_row_span(verts, cy, 0, 93);
                    for px in 0..94 {
                        let cx = px as f64 + 0.5;
                        let w0 = edge(v1.x, v1.y, v2.x, v2.y, cx, cy) * inv_area;
                        let w1 = edge(v2.x, v2.y, v0.x, v0.y, cx, cy) * inv_area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                            assert!(px >= left && px <= right, "pixel ({px},{py}) removed from {triangle:?}");
                        }
                    }
                }
            }
        }
        assert_eq!(conservative_row_span([V3::new(f64::NAN, 0.0, 0.0); 3], 10.5, 0, 93), (0, 93));
    }

    #[test]
    fn reused_depth_is_reset_across_poses_sizes_and_invalid_frames() {
        let textures = SkinTextures::decode(&png(64, 64), None).unwrap();
        let mut scratch = SkinRenderScratch::default();
        for (width, height, yaw) in [(94, 160, 0.0), (188, 320, 90.0), (0, 0, 0.0),
            (70, 120, -45.0), (94, 160, 180.0), (188, 320, 22.5)] {
            let fresh = render_skin_textures(&textures, SkinVariant::Classic, width, height, yaw, 10.5, 0.3, 0.0, 1.0);
            let reused = render_skin_textures_with_scratch(&textures, SkinVariant::Classic, width, height, yaw, 10.5, 0.3, 0.0, 1.0, &mut scratch);
            assert_eq!(fresh, reused);
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, image::Rgba([80, 120, 160, 255]));
        let mut output = Cursor::new(Vec::new());
        image.write_to(&mut output, ImageFormat::Png).unwrap();
        output.into_inner()
    }

    #[test]
    fn malformed_skin_dimensions_are_rejected_without_panicking() {
        for (width, height) in [(1, 1), (32, 64), (64, 48)] {
            let bytes = png(width, height);
            assert!(determine_skin_variant(&bytes).is_none());
            assert!(SkinTextures::decode(&bytes, None).is_none());
        }
    }

    #[test]
    fn cached_textures_match_single_frame_rendering() {
        for height in [32, 64] {
            let bytes = png(64, height);
            let textures = SkinTextures::decode(&bytes, None).unwrap();
            for variant in [SkinVariant::Classic, SkinVariant::Slim, SkinVariant::Other] {
                let cached = render_skin_textures(&textures, variant, 94, 160, 22.5, 10.5, 0.2, 0.0, 1.0);
                let decoded = render_skin_3d(&bytes, None, variant, 94, 160, 22.5, 10.5, 0.2, 0.0, 1.0);
                assert_eq!(cached, decoded);
            }
        }
    }

    #[test]
    fn invalid_framebuffer_sizes_are_rejected() {
        let textures = SkinTextures::decode(&png(64, 64), None).unwrap();
        for (width, height) in [(0, 160), (94, 0), (2049, 160), (94, 2049)] {
            assert!(render_skin_textures(&textures, SkinVariant::Classic, width, height, 0.0, 0.0, 0.0, 0.0, 1.0).is_none());
        }
    }

    #[test]
    fn transparent_face_culling_matches_full_rasterization() {
        for height in [32, 64] {
            let skin = RgbaImage::from_fn(64, height, |x, y| {
                let base = (16..32).contains(&y)
                    || (y >= 48 && (16..48).contains(&x))
                    || (y < 16 && x < 32);
                image::Rgba([(x * 3) as u8, (y * 4) as u8, 127, if base { 255 } else { 0 }])
            });
            let mut textures = SkinTextures {
                skin_alpha: AlphaCoverage::new(&skin),
                skin,
                cape: Some(RgbaImage::from_pixel(64, 32, image::Rgba([120, 80, 40, 128]))),
            };
            let full_coverage = AlphaCoverage::new(&RgbaImage::from_pixel(64, height, image::Rgba([0, 0, 0, 255])));
            for variant in [SkinVariant::Classic, SkinVariant::Slim, SkinVariant::Other] {
                for yaw in [-180.0, -90.0, -0.01, 0.0, 22.5, 90.0, 179.99] {
                    for zoom in [0.5, 1.0, 4.0] {
                        let culled = render_skin_textures(&textures, variant, 94, 160, yaw, 10.5, 0.3, 0.0, zoom);
                        let alpha = std::mem::replace(&mut textures.skin_alpha, AlphaCoverage {
                            stride: full_coverage.stride,
                            counts: full_coverage.counts.clone(),
                        });
                        let full = render_skin_textures(&textures, variant, 94, 160, yaw, 10.5, 0.3, 0.0, zoom);
                        textures.skin_alpha = alpha;
                        assert_eq!(culled, full, "height={height} yaw={yaw} zoom={zoom}");
                    }
                }
            }
        }
    }
}

// Debug function used to brute force the max bounds of the model
#[cfg(debug_assertions)]
pub fn brute_force_bounds() {
    let mut best_yaw = 0.0;
    let mut best_pitch = 0.0;
    let mut max_w = 0.0;
    let mut scale = 90.0;
    let yaw_acc = 128;
    let pitch_acc = 128;
    let mut yaw_offset = 0.0;
    let mut pitch_offset = 0.0;

    log::info!("Calculating largest width");
    loop {
        log::info!("Scale: {scale}");
        for y in 0..=yaw_acc {
            for p in 0..=pitch_acc {
                let yaw = y as f64 / yaw_acc as f64 * scale + yaw_offset;
                let pitch = p as f64 / pitch_acc as f64 * scale + pitch_offset;

                let projected_quads = collect_quads(false, true, true, yaw, pitch, MAX_CAPE_ANGLE_SWAY_PROGRESS);
                let mut w = f64::MIN;
                for quad in &projected_quads {
                    for vert in &quad.verts {
                        w = w.max(vert.x.abs());
                    }
                }

                if w * 2.0 > max_w {
                    max_w = w * 2.0;
                    best_yaw = yaw;
                    best_pitch = pitch;
                } else if w * 2.0 == max_w && best_yaw.abs() + best_pitch.abs() > yaw.abs() + pitch.abs() {
                    best_yaw = yaw;
                    best_pitch = pitch;
                }
            }
        }
        if scale == 90.0 {
            scale = 32.0;
        } else {
            scale /= 2.0;
        }
        let new_yaw_offset = best_yaw - scale / 2.0;
        let new_pitch_offset = best_pitch - scale / 2.0;
        if new_yaw_offset == yaw_offset || new_pitch_offset == pitch_offset {
            break;
        }
        yaw_offset = new_yaw_offset;
        pitch_offset = new_pitch_offset;
        log::info!("Best angle: {best_yaw:?}, {best_pitch:?}");
        log::info!("Width: {max_w:?}");
    }

    let best_w_yaw = best_yaw;
    let best_w_pitch = best_pitch;

    best_yaw = 0.0;
    best_pitch = 0.0;
    let mut max_h = 0.0;
    scale = 90.0;
    yaw_offset = 0.0;
    pitch_offset = 0.0;

    log::info!("Calculating largest height");
    loop {
        log::info!("Scale: {scale}");
        for y in 0..=yaw_acc {
            for p in 0..=pitch_acc {
                let yaw = y as f64 / yaw_acc as f64 * scale + yaw_offset;
                let pitch = p as f64 / pitch_acc as f64 * scale + pitch_offset;

                let projected_quads = collect_quads(false, true, true, yaw, pitch, MAX_CAPE_ANGLE_SWAY_PROGRESS);
                let mut h = f64::MIN;
                for quad in &projected_quads {
                    for vert in &quad.verts {
                        h = h.max(vert.y.abs());
                    }
                }

                if h * 2.0 > max_h {
                    max_h = h * 2.0;
                    best_yaw = yaw;
                    best_pitch = pitch;
                } else if h * 2.0 == max_h && best_yaw.abs() + best_pitch.abs() > yaw.abs() + pitch.abs() {
                    best_yaw = yaw;
                    best_pitch = pitch;
                }
            }
        }
        if scale == 90.0 {
            scale = 32.0;
        } else {
            scale /= 2.0;
        }
        let new_yaw_offset = best_yaw - scale / 2.0;
        let new_pitch_offset = best_pitch - scale / 2.0;
        if new_yaw_offset == yaw_offset || new_pitch_offset == pitch_offset {
            break;
        }
        yaw_offset = new_yaw_offset;
        pitch_offset = new_pitch_offset;
        log::info!("Best angle: {best_yaw:?}, {best_pitch:?}");
        log::info!("Height: {max_h:?}");
    }

    log::info!("Largest width = {max_w:?} (yaw={best_w_yaw:?}, pitch={best_w_pitch:?}");
    log::info!("Largest height = {max_h:?} (yaw={best_yaw:?}, pitch={best_pitch:?}");
}
