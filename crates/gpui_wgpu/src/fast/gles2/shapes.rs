use bytemuck::{Pod, Zeroable};
use gpui::{BorderStyle, Bounds, Corners, Edges, Hsla, ScaledPixels};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(super) struct Shape {
    pub size: [f32; 4],
    pub radii_x: [f32; 4],
    pub radii_y: [f32; 4],
    pub widths: [f32; 4],
    pub border: [f32; 4],
    pub mapping: [f32; 4],
}

pub(super) fn radii(corners: Corners<ScaledPixels>) -> [f32; 4] {
    [
        corners.top_left.0,
        corners.top_right.0,
        corners.bottom_right.0,
        corners.bottom_left.0,
    ]
}

impl Shape {
    pub fn new(
        bounds: Bounds<ScaledPixels>,
        horizontal: [f32; 4],
        vertical: [f32; 4],
        widths: Edges<ScaledPixels>,
        color: Hsla,
        style: BorderStyle,
        uv: [f32; 4],
    ) -> Self {
        let size = [bounds.size.width.0.max(0.0), bounds.size.height.0.max(0.0)];
        let mut rx = horizontal.map(|r| r.max(0.0));
        let mut ry = vertical.map(|r| r.max(0.0));
        let mut scale = 1.0_f32;
        for (side, sum) in [
            (size[0], rx[0] + rx[1]),
            (size[0], rx[3] + rx[2]),
            (size[1], ry[0] + ry[3]),
            (size[1], ry[1] + ry[2]),
        ] {
            if sum > 0.0 {
                scale = scale.min(side / sum);
            }
        }
        rx = rx.map(|r| r * scale);
        ry = ry.map(|r| r * scale);
        let border = color.to_rgb();
        let mapping = [
            size[0] / uv[2].max(f32::MIN_POSITIVE),
            size[1] / uv[3].max(f32::MIN_POSITIVE),
        ];
        Self {
            size: [
                size[0],
                size[1],
                if style == BorderStyle::Dashed {
                    1.0
                } else {
                    0.0
                },
                0.0,
            ],
            radii_x: rx,
            radii_y: ry,
            widths: [
                widths.top.0.max(0.0),
                widths.right.0.max(0.0),
                widths.bottom.0.max(0.0),
                widths.left.0.max(0.0),
            ],
            border: [
                border.r * border.a,
                border.g * border.a,
                border.b * border.a,
                border.a,
            ],
            mapping: [
                mapping[0],
                mapping[1],
                -uv[0] * mapping[0],
                -uv[1] * mapping[1],
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Shape;
    use gpui::{BorderStyle, Bounds, Edges, ScaledPixels, point, size, transparent_black};

    #[test]
    fn normalizes_overlapping_radii_and_maps_atlas_uv_to_local_pixels() {
        let shape = Shape::new(
            Bounds {
                origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
                size: size(ScaledPixels(100.0), ScaledPixels(40.0)),
            },
            [80.0; 4],
            [80.0; 4],
            Edges::default(),
            transparent_black(),
            BorderStyle::Solid,
            [0.25, 0.5, 0.125, 0.25],
        );
        assert_eq!(shape.radii_x, [20.0; 4]);
        assert_eq!(shape.radii_y, [20.0; 4]);
        assert_eq!(shape.mapping, [800.0, 160.0, -200.0, -80.0]);
        assert_eq!(shape.mapping[0] * 0.375 + shape.mapping[2], 100.0);
        assert_eq!(std::mem::size_of::<Shape>(), 96);
    }
}
