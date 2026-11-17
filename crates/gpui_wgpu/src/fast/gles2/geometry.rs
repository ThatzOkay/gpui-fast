use bytemuck::{Pod, Zeroable};
use gpui::{Bounds, Hsla, ScaledPixels, TransformationMatrix};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

pub(super) struct Rectangle {
    pub bounds: Bounds<ScaledPixels>,
    pub uv: [f32; 4],
    pub color: Hsla,
    pub transform: TransformationMatrix,
}

impl Rectangle {
    pub fn append(&self, viewport: [f32; 2], vertices: &mut Vec<Vertex>) {
        let color = self.color.to_rgb();
        for [x, y] in [
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [0.0, 1.0],
            [1.0, 0.0],
            [1.0, 1.0],
        ] {
            let local = [
                self.bounds.origin.x.0 + x * self.bounds.size.width.0,
                self.bounds.origin.y.0 + y * self.bounds.size.height.0,
            ];
            let m = self.transform.rotation_scale;
            let pixel = [
                m[0][0] * local[0] + m[0][1] * local[1] + self.transform.translation[0],
                m[1][0] * local[0] + m[1][1] * local[1] + self.transform.translation[1],
            ];
            vertices.push(Vertex {
                position: [
                    2.0 * pixel[0] / viewport[0] - 1.0,
                    1.0 - 2.0 * pixel[1] / viewport[1],
                ],
                uv: [self.uv[0] + x * self.uv[2], self.uv[1] + y * self.uv[3]],
                color: [
                    color.r * color.a,
                    color.g * color.a,
                    color.b * color.a,
                    color.a,
                ],
            });
        }
    }
}

pub(super) fn scissor(bounds: Bounds<ScaledPixels>, viewport: [f32; 2]) -> Option<[u32; 4]> {
    let edge = |value: f32, limit: f32| (value - 0.5).ceil().clamp(0.0, limit) as u32;
    let left = edge(bounds.origin.x.0, viewport[0]);
    let top = edge(bounds.origin.y.0, viewport[1]);
    let right = edge(bounds.right().0, viewport[0]);
    let bottom = edge(bounds.bottom().0, viewport[1]);
    (right > left && bottom > top).then(|| [left, top, right - left, bottom - top])
}

#[cfg(test)]
mod tests {
    use super::{Rectangle, Vertex, scissor};
    use gpui::{Bounds, ScaledPixels, TransformationMatrix, point, size, white};

    #[test]
    fn rectangle_maps_pixels_and_atlas_coordinates() {
        let rect = Rectangle {
            bounds: Bounds {
                origin: point(ScaledPixels(25.0), ScaledPixels(10.0)),
                size: size(ScaledPixels(50.0), ScaledPixels(20.0)),
            },
            uv: [0.25, 0.5, 0.125, 0.25],
            color: white(),
            transform: TransformationMatrix::unit(),
        };
        let mut vertices: Vec<Vertex> = Vec::new();
        rect.append([100.0, 40.0], &mut vertices);
        assert_eq!(vertices.len(), 6);
        assert_eq!(vertices[0].position, [-0.5, 0.5]);
        assert_eq!(vertices[5].position, [0.5, -0.5]);
        assert_eq!(vertices[0].uv, [0.25, 0.5]);
        assert_eq!(vertices[5].uv, [0.375, 0.75]);
        assert_eq!(vertices[0].color, [1.0; 4]);

        let transformed = Rectangle {
            transform: TransformationMatrix {
                rotation_scale: [[0.0, -1.0], [1.0, 0.0]],
                translation: [40.0, 5.0],
            },
            ..rect
        };
        vertices.clear();
        transformed.append([100.0, 40.0], &mut vertices);
        assert!((vertices[0].position[0] + 0.4).abs() < 0.00001);
        assert_eq!(vertices[0].position[1], -0.5);
        assert!((vertices[5].position[0] + 0.8).abs() < 0.00001);
        assert_eq!(vertices[5].position[1], -3.0);
    }
    #[test]
    fn scissor_clamps_to_viewport_and_excludes_empty_masks() {
        let bounds = Bounds {
            origin: point(ScaledPixels(-20.0), ScaledPixels(10.0)),
            size: size(ScaledPixels(40.0), ScaledPixels(80.0)),
        };
        assert_eq!(scissor(bounds, [100.0, 40.0]), Some([0, 10, 20, 30]));
        let outside = Bounds {
            origin: point(ScaledPixels(110.0), ScaledPixels(0.0)),
            size: size(ScaledPixels(10.0), ScaledPixels(10.0)),
        };
        assert_eq!(scissor(outside, [100.0, 40.0]), None);
    }

    #[test]
    fn fractional_scissor_tests_pixel_centers() {
        let bounds = Bounds {
            origin: point(ScaledPixels(1.5), ScaledPixels(1.6)),
            size: size(ScaledPixels(3.0), ScaledPixels(3.0)),
        };
        assert_eq!(scissor(bounds, [100.0, 40.0]), Some([1, 2, 3, 3]));
    }
}
