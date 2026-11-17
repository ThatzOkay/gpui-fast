struct Varying {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

@vertex fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> Varying {
    return Varying(vec4<f32>(position, 0.0, 1.0), uv, color);
}

@fragment fn fs_solid(v: Varying) -> @location(0) vec4<f32> {
    return v.color;
}

@fragment fn fs_mono(v: Varying) -> @location(0) vec4<f32> {
    return v.color * textureSample(image, image_sampler, v.uv).r;
}

struct Shape {
    size: vec4<f32>,
    radii_x: vec4<f32>,
    radii_y: vec4<f32>,
    widths: vec4<f32>,
    border: vec4<f32>,
    mapping: vec4<f32>,
};
@group(1) @binding(0) var<uniform> shape: Shape;

fn corner_value(p: vec2<f32>, half_size: vec2<f32>, values: vec4<f32>) -> f32 {
    if (p.y < half_size.y) {
        if (p.x < half_size.x) { return values.x; }
        return values.y;
    }
    if (p.x < half_size.x) { return values.w; }
    return values.z;
}

fn ellipse_rect_distance(p: vec2<f32>, size: vec2<f32>, rx: f32, ry: f32) -> f32 {
    let half_size = max(size * 0.5, vec2<f32>(0.0));
    let r = min(vec2<f32>(rx, ry), half_size);
    let centered = abs(p - half_size);
    let q = centered - (half_size - r);
    if (r.x < 0.0001 || r.y < 0.0001 || q.x <= 0.0 || q.y <= 0.0) {
        let d = centered - half_size;
        return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
    }
    let k0 = length(q / r);
    let k1 = length((q / r) / r);
    return k0 * (k0 - 1.0) / max(k1, 0.0001);
}

fn outer_coverage(uv: vec2<f32>) -> f32 {
    let p = uv * shape.mapping.xy + shape.mapping.zw;
    let half_size = shape.size.xy * 0.5;
    let rx = corner_value(p, half_size, shape.radii_x);
    let ry = corner_value(p, half_size, shape.radii_y);
    return clamp(0.5 - ellipse_rect_distance(p, shape.size.xy, rx, ry), 0.0, 1.0);
}

fn dash_coverage(p: vec2<f32>, size: vec2<f32>) -> f32 {
    let near_horizontal = min(p.y, size.y - p.y) <= min(p.x, size.x - p.x);
    let horizontal = select(2.0 * size.x + size.y - p.x, p.x, p.y < size.y * 0.5);
    let vertical = select(2.0 * (size.x + size.y) - p.y, size.x + p.y, p.x > size.x * 0.5);
    let along = select(vertical, horizontal, near_horizontal);
    let width = max(max(max(shape.widths.x, shape.widths.y), max(shape.widths.z, shape.widths.w)), 0.5);
    let period = 6.0 * width;
    let center_distance = abs(fract(along / period + 0.5) - 0.5) * period;
    return 1.0 - smoothstep(period * 0.25 - 0.5, period * 0.25 + 0.5, center_distance);
}

@fragment fn fs_rounded(v: Varying) -> @location(0) vec4<f32> {
    return v.color * outer_coverage(v.uv);
}

fn border_coverage(uv: vec2<f32>) -> f32 {
    let p = uv * shape.mapping.xy + shape.mapping.zw;
    let half_size = shape.size.xy * 0.5;
    let horizontal = select(shape.widths.w, shape.widths.y, p.x >= half_size.x);
    let vertical = select(shape.widths.x, shape.widths.z, p.y >= half_size.y);
    let rx = corner_value(p, half_size, shape.radii_x);
    let ry = corner_value(p, half_size, shape.radii_y);
    let inner_size = shape.size.xy - vec2<f32>(shape.widths.w + shape.widths.y, shape.widths.x + shape.widths.z);
    let inner_position = p - vec2<f32>(shape.widths.w, shape.widths.x);
    let distance = ellipse_rect_distance(inner_position, max(inner_size, vec2<f32>(0.001)), max(rx - horizontal, 0.0), max(ry - vertical, 0.0));
    return select(1.0, clamp(0.5 + distance, 0.0, 1.0), all(inner_size > vec2<f32>(0.0)));
}
@fragment fn fs_border(v: Varying) -> @location(0) vec4<f32> {
    return v.color * outer_coverage(v.uv) * border_coverage(v.uv);
}
@fragment fn fs_border_dashed(v: Varying) -> @location(0) vec4<f32> {
    let p = v.uv * shape.mapping.xy + shape.mapping.zw;
    let dash = dash_coverage(p, shape.size.xy);
    return textureSample(image, image_sampler, v.uv) * v.color * dash;
}

fn sampled_color(v: Varying) -> vec4<f32> {
    let sample = textureSample(image, image_sampler, v.uv);
    return vec4<f32>(sample.rgb * sample.a, sample.a) * v.color;
}

@fragment fn fs_color(v: Varying) -> @location(0) vec4<f32> { return sampled_color(v); }
@fragment fn fs_color_gray(v: Varying) -> @location(0) vec4<f32> {
    let color = sampled_color(v);
    let grey = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec4<f32>(vec3<f32>(grey), color.a);
}
@fragment fn fs_color_rounded(v: Varying) -> @location(0) vec4<f32> { return sampled_color(v) * outer_coverage(v.uv); }
@fragment fn fs_color_rounded_gray(v: Varying) -> @location(0) vec4<f32> {
    let color = sampled_color(v);
    let grey = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec4<f32>(vec3<f32>(grey), color.a) * outer_coverage(v.uv);
}
@fragment fn fs_premult(v: Varying) -> @location(0) vec4<f32> { return textureSample(image, image_sampler, v.uv) * v.color; }
@fragment fn fs_premult_gray(v: Varying) -> @location(0) vec4<f32> {
    let color = textureSample(image, image_sampler, v.uv) * v.color;
    let grey = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec4<f32>(vec3<f32>(grey), color.a);
}
@fragment fn fs_premult_rounded(v: Varying) -> @location(0) vec4<f32> { return textureSample(image, image_sampler, v.uv) * v.color * outer_coverage(v.uv); }

@fragment fn fs_premult_rounded_gray(v: Varying) -> @location(0) vec4<f32> {
    let color = textureSample(image, image_sampler, v.uv) * v.color;
    let grey = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec4<f32>(vec3<f32>(grey), color.a) * outer_coverage(v.uv);
}
